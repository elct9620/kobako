//! Resolving a path beneath a directory, one name at a time, and the
//! calls that act on the name it ends at: opening it, making it, removing
//! it, and moving it, as Linux's `namei.c` holds both.
//!
//! Each step hands a Filesystem one name in a directory already held,
//! opened without following a link there, so nothing another writer swaps
//! in on the way leads the walk elsewhere. A link is followed only where
//! the walk itself reads where it points, within the directory the walk
//! started from, as `openat2`'s `RESOLVE_BENEATH` keeps a resolution: one
//! leading out of it, or to an absolute path, escapes it. What else a link may
//! do is for the security hooks to answer.

use std::collections::VecDeque;
use std::sync::Arc;

use super::cred::Cred;
use super::dentry::Dentry;
use super::file::{File, FileOperations};
use super::inode::{Inode, RenameFlag};
use super::mount::{Access, VfsMount};
use super::openat2::{build_open_flags, do_truncate, vfs_open, OpenHow, OpenOptions};
use super::path::Path;
use super::super_block::SuperBlock;
use super::{Error, FileType, FilesystemError, Iattr, Metadata};
use crate::locking::RwSemaphoreGuard;

use super::PATH_MAX;

/// The most links one walk follows, as Linux's.
const MAXSYMLINKS: usize = 40;

/// What the last name of a path is, as Linux's `nd->last_type`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum LastType {
    Norm,
    Dot,
    DotDot,
}

/// A path walked beneath a directory, as Linux's `nameidata` carries
/// one walk from where it starts to the call it ends in: the directories
/// it stands in, the names still to walk, and the last name, empty for
/// the directory itself, left for the call to act on.
pub(crate) struct Nameidata {
    pub(crate) mnt: Arc<VfsMount>,
    pub(crate) cred: Arc<Cred>,
    /// Each directory from the one the walk started from, which no `..`
    /// climbs above, down to the one it stands in.
    frames: Vec<Frame>,
    pending: VecDeque<Part>,
    links: usize,
    /// The last name, as Linux's `nd->last`, empty unless it is a `Norm` one.
    pub(crate) name: String,
    last_type: LastType,
    /// Whether a `/` follows the last name, as `nd->last.name[nd->last.len]`.
    trailing_slash: bool,
    /// Whether a link's target named the last name.
    last_from_link: bool,
    /// The dentry the walk looked `name` up on, kept for as long as the
    /// walk runs, as Linux's `nameidata` holds what it found, so what the
    /// lookup recorded there is what the call acts on.
    last: Option<Arc<Dentry>>,
    /// Whether only a directory satisfies the path, as `LOOKUP_DIRECTORY`,
    /// which stays once a trailing `/` sets it, through any link it follows.
    lookup_directory: bool,
    /// Whether the call acts on the last name rather than land on it, as
    /// `LOOKUP_PARENT`, so a `.` or `..` there takes the walk nowhere.
    lookup_parent: bool,
    /// What kind of entry stands at `name`, a link reported as itself, once
    /// asked.
    found: Option<Result<FileType, FilesystemError>>,
    /// Whether a link at the end of the path led to `name`.
    followed_link: bool,
}

impl Nameidata {
    /// The directory the walk stands in, which holds the last name.
    pub(crate) fn dentry(&self) -> &Arc<Dentry> {
        &self.frames.last().expect("a walk stands somewhere").dentry
    }

    /// The dentry of the last name in the directory the walk stands in, looked
    /// up afresh, as a change looks its name up again once it holds the
    /// directory's lock.
    pub(crate) fn child(&self) -> Result<Arc<Dentry>, FilesystemError> {
        lookup_one(self.mnt.sb(), self.dentry(), &self.name)
    }

    /// The dentry the walk ended on, as Linux's `do_open` and `vfs_getattr`
    /// act on what the walk found without looking it up again, so a change
    /// made since cannot lead the call anywhere but where the walk stands.
    pub(crate) fn last_dentry(&self) -> Result<Arc<Dentry>, FilesystemError> {
        match &self.last {
            Some(dentry) => Ok(dentry.clone()),
            None => self.child(),
        }
    }

    /// `name` as the directory the walk stands in stores it, and the dentry of
    /// that spelling, what stands there recorded on it. The caller holds that
    /// directory's lock. A directory removed through the VFS holds no name any
    /// more, as Linux's lookup in a dead directory finds nothing.
    fn lookup_slow(&self, name: &str) -> Result<(String, Arc<Dentry>), FilesystemError> {
        if self.dentry().is_deaddir() {
            return Err(FilesystemError::NotFound);
        }
        lookup_slow(self.mnt.sb(), self.dentry(), name)
    }

    /// Find the last name afresh and keep what stands there, as a call looks
    /// its name up once it holds the directory's lock.
    fn lookup_last_locked(&mut self) -> Result<Arc<Dentry>, FilesystemError> {
        let (stored, child) = self.lookup_slow(&self.name)?;
        self.name = stored;
        self.found = Some(child.kind());
        self.last = Some(child.clone());
        Ok(child)
    }

    /// What kind of entry stands at the name, a link reported as itself,
    /// read from the dentry as Linux's `d_is_*` do; the directory itself for
    /// the empty name.
    pub(crate) fn kind(&mut self) -> Result<FileType, FilesystemError> {
        if self.found.is_none() {
            let parent = self.dentry().clone();
            self.found = Some(parent.inode_lock_shared().and_then(|_held| {
                let child = self.last_dentry()?;
                self.look_up(&child)
            }));
        }
        self.found.clone().expect("found was just asked")
    }

    /// What kind of entry stands at the name, whose dentry is `child`, a
    /// link reported as itself; the directory itself for the empty name. A
    /// dentry naming nothing is looked up again, as the caller may have just
    /// taken the lock a change was holding.
    fn look_up(&self, child: &Dentry) -> Result<FileType, FilesystemError> {
        match self.name.as_str() {
            "" => Ok(self.dentry().getattr()?.file_type),
            _ => {
                child.lookup_inode(self.dentry())?;
                child.kind()
            }
        }
    }

    /// What stands at the name reports of itself, a link reported as itself,
    /// as `vfs_getattr` asks the entry the walk found.
    pub(crate) fn getattr(&mut self) -> Result<Metadata, FilesystemError> {
        if self.name.is_empty() {
            let parent = self.dentry().clone();
            let _held = parent.inode_lock_shared()?;
            return parent.getattr();
        }
        self.kind()?;
        let parent = self.dentry().clone();
        let _held = parent.inode_lock_shared()?;
        self.last_dentry()?.getattr()
    }

    /// Refuse the name as `NotADirectory` when it must be a directory and
    /// something else stands there, as `LOOKUP_DIRECTORY` asks `d_can_lookup`.
    fn check_directory(&mut self) -> Result<(), Error> {
        if self.lookup_directory {
            match self.kind() {
                Ok(FileType::Directory) | Err(FilesystemError::NotFound) => {}
                Ok(_) => return Err(FilesystemError::NotADirectory.into()),
                Err(err) => return Err(err.into()),
            }
        }
        Ok(())
    }

    /// The path from the Mount's root, read from the dentry the walk stands
    /// in as Linux's `d_path` reads one: each name spelled as the directory
    /// holding it stores it, the last name kept as given where nothing
    /// holds it.
    pub(crate) fn dentry_path(&self) -> String {
        let mut names = self.dentry().names();
        names.push(self.name.clone());
        names.retain(|name| !name.is_empty());
        names.join("/")
    }

    /// Whether the path may be reached with `access`, as an open asks: a change
    /// asks the mount first, then the security hook.
    fn check(&self, access: Access) -> Result<(), Error> {
        if access == Access::ReadWrite {
            self.mnt.mnt_want_write()?;
        }
        self.security_path_perm(access)
    }

    /// Whether a security hook lets the path be reached with `access`, as
    /// Linux's `security_path_*` asks once its call holds the name.
    fn security_path_perm(&self, access: Access) -> Result<(), Error> {
        self.cred
            .security_path_perm(&self.mnt, &|| self.dentry_path(), access)
    }
}

/// The dentry of `name` in `parent`, as Linux's `lookup_one` finds one: a dentry
/// already held is asked whether it still names what stands there, as
/// `lookup_fast` asks `d_revalidate`, and let go of for a new one where it does
/// not.
pub(crate) fn lookup_one(
    sb: &SuperBlock,
    parent: &Arc<Dentry>,
    name: &str,
) -> Result<Arc<Dentry>, FilesystemError> {
    let dentries = sb.dentries();
    if let Some(held) = dentries.lookup(parent, name) {
        let valid = match held.ops() {
            Some(ops) => ops.revalidate(parent.inode_ops()?.as_ref(), &held)?,
            None => true,
        };
        if valid {
            return Ok(held);
        }
        dentries.unhash(&held);
    }
    Ok(dentries.child(parent, name))
}

/// `name` in `parent` as that directory stores it, and the dentry of that
/// spelling with what stands there recorded on it, as Linux's `lookup_slow` asks
/// `->lookup` and a case-insensitive filesystem answers through `d_add_ci`:
/// every spelling reaching one entry shares its dentry, and so its lock.
pub(crate) fn lookup_slow(
    sb: &SuperBlock,
    parent: &Arc<Dentry>,
    name: &str,
) -> Result<(String, Arc<Dentry>), FilesystemError> {
    let given = lookup_one(sb, parent, name)?;
    let (stored, dentry) = match given.lookup_inode(parent)? {
        None => (name.to_string(), given),
        Some(stored) => {
            let dentry = lookup_one(sb, parent, &stored)?;
            dentry.lookup_inode(parent)?;
            (stored, dentry)
        }
    };
    sb.dentries().retain(&dentry);
    Ok((stored, dentry))
}

/// One name of a path still to walk, and whether a link's target named it.
struct Part {
    name: String,
    from_link: bool,
}

fn parts(path: &str, from_link: bool) -> VecDeque<Part> {
    path.split('/')
        .map(|name| Part {
            name: name.to_string(),
            from_link,
        })
        .collect()
}

/// A directory the walk stands in, and whether a link led to it.
struct Frame {
    dentry: Arc<Dentry>,
    through_link: bool,
}

impl Nameidata {
    /// Step into the directory `part` names, or follow the link standing
    /// there; `dot` is a `.` or `..` right after it, which no link takes.
    fn walk_component(&mut self, part: Part, dot: bool) -> Result<(), Error> {
        let parent = self.dentry().clone();
        let _held = parent.inode_lock_shared()?;
        let (_, dentry) = self.lookup_slow(&part.name)?;
        let inode = dentry.inode().ok_or(FilesystemError::NotFound)?;
        match inode.kind() {
            FileType::Symlink => {
                if dot {
                    self.cred.security_path_step_out_of_link()?;
                }
                self.pick_link(&dentry)
            }
            FileType::Directory => {
                self.frames.push(Frame {
                    dentry,
                    through_link: part.from_link,
                });
                Ok(())
            }
            _ => Err(FilesystemError::NotADirectory.into()),
        }
    }

    /// Walk on through where the link `dentry` names points, before
    /// whatever is left of the path.
    fn pick_link(&mut self, dentry: &Dentry) -> Result<(), Error> {
        self.cred.security_inode_follow_link(&self.mnt)?;
        self.links += 1;
        if self.links > MAXSYMLINKS {
            return Err(FilesystemError::FilesystemLoop.into());
        }
        let target = dentry.get_link()?;
        if target.starts_with('/') {
            return Err(Error::Escape);
        }
        if target.is_empty() {
            return Err(FilesystemError::NotFound.into());
        }
        for part in parts(&target, true).into_iter().rev() {
            self.pending.push_front(part);
        }
        Ok(())
    }

    /// Step out of the directory the walk stands in. Out of the one the
    /// walk started from, no `..` holds a capability, as `RESOLVE_BENEATH`
    /// refuses a caller's and a link's alike; out of one a link led to, the
    /// security hooks are asked.
    fn follow_dotdot(&mut self, from_link: bool) -> Result<(), Error> {
        if self.frames.len() == 1 {
            return Err(Error::Escape);
        }
        if !from_link && self.frames.last().is_some_and(|frame| frame.through_link) {
            self.cred.security_path_step_out_of_link()?;
        }
        self.frames.pop();
        Ok(())
    }

    /// Walk every name but the last, which is left for the call to act on, as
    /// Linux's `link_path_walk` leaves `nd->last` and `nd->last_type`.
    fn link_path_walk(&mut self) -> Result<(), Error> {
        loop {
            let part = self.pending.pop_front().ok_or(FilesystemError::NotFound)?;
            if part.name.is_empty() {
                if self.pending.is_empty() {
                    self.set_last("", part.from_link, true);
                    return Ok(());
                }
                continue;
            }
            let slashes = self
                .pending
                .iter()
                .take_while(|rest| rest.name.is_empty())
                .count();
            self.pending.drain(..slashes);
            if self.pending.is_empty() {
                self.set_last(&part.name, part.from_link, slashes > 0);
                return Ok(());
            }
            match part.name.as_str() {
                "." => {}
                ".." => self.follow_dotdot(part.from_link)?,
                _ => {
                    let lands = !self.lookup_parent
                        || self
                            .pending
                            .iter()
                            .skip(1)
                            .any(|rest| !rest.name.is_empty());
                    let dot = lands && matches!(self.pending[0].name.as_str(), "." | "..");
                    self.walk_component(part, dot)?;
                }
            }
        }
    }

    /// Leave `name` as the last name of the walk, with what kind it is.
    fn set_last(&mut self, name: &str, from_link: bool, trailing_slash: bool) {
        self.last_type = match name {
            "" | "." => LastType::Dot,
            ".." => LastType::DotDot,
            _ => LastType::Norm,
        };
        self.name = match self.last_type {
            LastType::Norm => name.to_string(),
            _ => String::new(),
        };
        self.last_from_link = from_link;
        self.trailing_slash = trailing_slash;
    }

    /// Find what the last name stands for, as Linux's `lookup_last` does: a
    /// trailing `/` follows a link there and asks for a directory, and a link
    /// followed leaves another last name to find. A trailing `/` refuses a file
    /// `creating` would make, as `lookup_fast_for_open` refuses it.
    fn lookup_last(&mut self, mut follow: bool, creating: bool) -> Result<(), Error> {
        loop {
            match self.last_type {
                LastType::Dot => return Ok(()),
                LastType::DotDot => return self.follow_dotdot(self.last_from_link),
                LastType::Norm => {}
            }
            if self.trailing_slash {
                if creating {
                    return Err(FilesystemError::IsADirectory.into());
                }
                follow = true;
                self.lookup_directory = true;
            }
            let parent = self.dentry().clone();
            let held = parent.inode_lock_shared()?;
            let child = self.lookup_last_locked()?;
            if !follow || child.kind() != Ok(FileType::Symlink) {
                return Ok(());
            }
            self.pick_link(&child)?;
            drop(held);
            self.followed_link = true;
            self.last = None;
            self.found = None;
            self.link_path_walk()?;
        }
    }
}

/// Refuse a path no call can take, as Linux's `getname` does before any call
/// looks at it: an empty one names nothing, one past `PATH_MAX` with its
/// terminating NUL is too long, and one holding a NUL names nothing a
/// Filesystem can hold.
fn getname(path: &str) -> Result<(), FilesystemError> {
    if path.is_empty() {
        return Err(FilesystemError::NotFound);
    }
    if path.len() >= PATH_MAX {
        return Err(FilesystemError::InvalidFilename);
    }
    if path.contains('\0') {
        return Err(FilesystemError::InvalidInput);
    }
    Ok(())
}

/// The walk of `path` beneath `start`, ready to begin, as Linux's
/// `set_nameidata` and `path_init` make one. An empty path names nothing, and an
/// absolute one is not the caller's to reach.
fn set_nameidata(cred: &Arc<Cred>, start: &Path, path: &str) -> Result<Nameidata, Error> {
    getname(path)?;
    cred.security_path_walk(start)?;
    if path.starts_with('/') {
        return Err(Error::Escape);
    }
    Ok(Nameidata {
        mnt: start.mnt().clone(),
        cred: cred.clone(),
        frames: vec![Frame {
            dentry: start.dentry().clone(),
            through_link: false,
        }],
        pending: parts(path, false),
        links: 0,
        name: String::new(),
        last_type: LastType::Dot,
        trailing_slash: false,
        last_from_link: false,
        last: None,
        lookup_directory: false,
        lookup_parent: false,
        found: None,
        followed_link: false,
    })
}

/// `path` beneath `start`, walked to the directory holding its last name, which
/// is left unlooked up for the call to act on, as Linux's `filename_parentat`.
/// A `..` above `start` is not the caller's to reach.
fn filename_parentat(cred: &Arc<Cred>, start: &Path, path: &str) -> Result<Nameidata, Error> {
    let mut nd = set_nameidata(cred, start, path)?;
    nd.lookup_parent = true;
    nd.link_path_walk()?;
    Ok(nd)
}

/// `path` beneath `start`, walked to what it names, as Linux's `filename_lookup`.
/// Where `follow` asks, or a trailing `/` does, a link at the end is followed
/// too. Nothing standing at the last name is for the call to answer.
pub(crate) fn filename_lookup(
    cred: &Arc<Cred>,
    start: &Path,
    path: &str,
    follow: bool,
) -> Result<Nameidata, Error> {
    let mut nd = set_nameidata(cred, start, path)?;
    nd.link_path_walk()?;
    nd.lookup_last(follow, false)?;
    nd.check_directory()?;
    Ok(nd)
}

/// Open `path` beneath `start` as `how` asks, as Linux's `do_file_open`
/// walks to the last name and `do_open` opens it. It hands no descriptor out,
/// as whoever holds a table of them reserves one first, as Linux's `FD_ADD`
/// does.
pub fn do_file_open(
    cred: &Arc<Cred>,
    start: &Path,
    path: &str,
    how: &OpenHow,
) -> Result<File, Error> {
    let follow = build_open_flags(how)?;
    let mut nd = set_nameidata(cred, start, path)?;
    nd.link_path_walk()?;
    nd.lookup_last(follow, how.create)?;
    // A create finds what stands at the name under the directory's lock it
    // creates under, and holds it until the file is open, as Linux's
    // `lookup_open` does, so nothing made or removed in between changes
    // what the open decided.
    let parent = nd.dentry().clone();
    let _held = match how.create && nd.last_type == LastType::Norm {
        true => {
            let held = parent.inode_lock()?;
            nd.lookup_last_locked()?;
            Some(held)
        }
        false => None,
    };
    // The walk followed every link the call asked it to, so a link at the
    // end now is one swapped in since, and is opened as itself.
    nd.check_directory()?;
    let existing = match nd.kind() {
        Ok(kind) => Some(kind),
        Err(FilesystemError::NotFound) => None,
        Err(err) => return Err(err.into()),
    };
    if how.create && existing.is_none() {
        cred.security_path_mknod(nd.followed_link)?;
    }
    refuse_what_stands(existing, how)?;
    // Creating changes the Mount only where nothing stands yet, so the
    // Mount judges the open once, knowing what it does.
    let changes = how.write || how.truncate || (how.create && existing.is_none());
    nd.check(if changes {
        Access::ReadWrite
    } else {
        Access::Read
    })?;
    if let Some(kind) = existing {
        cred.security_file_open(kind)?;
    }
    match existing {
        Some(FileType::Directory) => vfs_open(start, nd, how),
        Some(_) | None => {
            let options = OpenOptions {
                read: how.read,
                write: how.write,
                truncate: how.truncate,
            };
            let child = nd.last_dentry()?;
            let f_op = match existing {
                None => atomic_open(nd.dentry(), &child, &options)?,
                Some(_) => child.open(&options)?,
            };
            // Only what stood is emptied, as Linux's `do_open` calls
            // `handle_truncate` for a regular file it did not make.
            if how.truncate && existing == Some(FileType::File) {
                do_truncate(&child, &Iattr::open_truncate())?;
            }
            Ok(File::new(start.with_dentry(child), cred.clone(), f_op, how))
        }
    }
}

/// What stands at the path answers before any access is judged, as Linux's
/// `do_open` answers it on a read-only mount.
fn refuse_what_stands(existing: Option<FileType>, how: &OpenHow) -> Result<(), Error> {
    let refusal = match existing {
        None if !how.create => Some(FilesystemError::NotFound),
        Some(_) if how.create && how.exclusive => Some(FilesystemError::AlreadyExists),
        Some(FileType::Directory) if how.create => Some(FilesystemError::IsADirectory),
        Some(FileType::Directory) | None => None,
        Some(_) if how.directory => Some(FilesystemError::NotADirectory),
        Some(_) => None,
    };
    match refusal {
        Some(error) => Err(error.into()),
        None => may_open(existing, how),
    }
}

/// Refuse to open what stands at the path as Linux's `may_open` does: a
/// link, which the walk would have followed if asked, and a directory for
/// writing.
fn may_open(existing: Option<FileType>, how: &OpenHow) -> Result<(), Error> {
    match existing {
        Some(FileType::Symlink) => Err(FilesystemError::FilesystemLoop.into()),
        Some(FileType::Directory) if how.write || how.truncate => {
            Err(FilesystemError::IsADirectory.into())
        }
        _ => Ok(()),
    }
}

/// Give what `old_path` beneath `old_start` names another name at `new_path`
/// beneath `new_start`, as Linux's `filename_linkat` walks to the first, then
/// to where the second is made, and refuses a move across mounts once it knows
/// nothing stands there. A link at the end of `old_path` is followed where
/// `follow` asks.
pub fn filename_linkat(
    cred: &Arc<Cred>,
    old_start: &Path,
    old_path: &str,
    new_start: &Path,
    new_path: &str,
    follow: bool,
) -> Result<(), Error> {
    let mut old = filename_lookup(cred, old_start, old_path, follow)?;
    old.kind()?;
    let target = match old.name.is_empty() {
        true => old.dentry().clone(),
        false => old.last_dentry()?,
    };
    let mut new = filename_parentat(cred, new_start, new_path)?;
    let (_held, child) = filename_create(&mut new, false)?;
    if !Arc::ptr_eq(old_start.mnt(), new_start.mnt()) {
        return Err(FilesystemError::CrossesDevices.into());
    }
    old.security_path_perm(Access::ReadWrite)?;
    new.security_path_perm(Access::ReadWrite)?;
    Ok(vfs_link(&target, new.dentry(), &child)?)
}

/// Make the link `path` beneath `start` names, pointing at `target`, as
/// Linux's `filename_symlinkat` does.
pub fn filename_symlinkat(
    cred: &Arc<Cred>,
    target: &str,
    start: &Path,
    path: &str,
) -> Result<(), Error> {
    getname(target)?;
    let mut nd = filename_parentat(cred, start, path)?;
    let (_held, child) = filename_create(&mut nd, false)?;
    nd.security_path_perm(Access::ReadWrite)?;
    Ok(vfs_symlink(nd.dentry(), &child, target)?)
}

/// Hold the directory a name is made in and take the name's dentry, as
/// Linux's `filename_create` does: a `.` or `..` already stands, and what stands
/// there answers before the mount refuses a change, so a call making what
/// already stands learns so even beneath a read-only mount. A trailing `/` on
/// anything but a directory being made finds nothing there to be one.
fn filename_create(
    nd: &mut Nameidata,
    want_dir: bool,
) -> Result<(RwSemaphoreGuard, Arc<Dentry>), Error> {
    if nd.last_type != LastType::Norm {
        return Err(FilesystemError::AlreadyExists.into());
    }
    let write = nd.mnt.mnt_want_write();
    let held = nd.dentry().inode_lock()?;
    let child = nd.lookup_last_locked()?;
    may_create_dentry(nd.dentry(), &child)?;
    if nd.trailing_slash && !want_dir {
        return Err(FilesystemError::NotFound.into());
    }
    write?;
    Ok((held, child))
}

/// Hold the directory a name is removed from and take the name's dentry, as
/// Linux's `start_dirop` does before any security hook is asked.
fn start_dirop(nd: &mut Nameidata) -> Result<(RwSemaphoreGuard, Arc<Dentry>), Error> {
    let held = nd.dentry().inode_lock()?;
    let child = nd.lookup_last_locked()?;
    match child.inode() {
        Some(_) => Ok((held, child)),
        None => Err(FilesystemError::NotFound.into()),
    }
}

/// Make the directory `path` beneath `start`. A trailing `/` asks for the
/// directory being made, so it is no refusal here, as `mkdir(2)` takes it.
pub fn filename_mkdirat(cred: &Arc<Cred>, start: &Path, path: &str) -> Result<(), Error> {
    let mut nd = filename_parentat(cred, start, path)?;
    let (_held, child) = filename_create(&mut nd, true)?;
    nd.security_path_perm(Access::ReadWrite)?;
    Ok(vfs_mkdir(nd.dentry(), &child)?)
}

/// Remove the empty directory `path` beneath `start`, as Linux's
/// `filename_rmdir` does.
pub fn filename_rmdir(cred: &Arc<Cred>, start: &Path, path: &str) -> Result<(), Error> {
    let mut nd = filename_parentat(cred, start, path)?;
    match nd.last_type {
        LastType::Norm => {}
        LastType::Dot => return Err(FilesystemError::InvalidInput.into()),
        LastType::DotDot => return Err(FilesystemError::DirectoryNotEmpty.into()),
    }
    nd.mnt.mnt_want_write()?;
    let (_held, child) = start_dirop(&mut nd)?;
    nd.security_path_perm(Access::ReadWrite)?;
    Ok(vfs_rmdir(nd.mnt.sb(), nd.dentry(), &child)?)
}

/// Remove the file `path` beneath `start`, as Linux's `filename_unlinkat` does.
/// It follows no link at the end, so a trailing `/` is answered by the name
/// itself once it is found: no file removes as a directory.
pub fn filename_unlinkat(cred: &Arc<Cred>, start: &Path, path: &str) -> Result<(), Error> {
    let mut nd = filename_parentat(cred, start, path)?;
    if nd.last_type != LastType::Norm {
        return Err(FilesystemError::IsADirectory.into());
    }
    nd.mnt.mnt_want_write()?;
    let (_held, child) = start_dirop(&mut nd)?;
    if nd.trailing_slash {
        return Err(match child.kind() {
            Ok(FileType::Directory) => FilesystemError::IsADirectory,
            _ => FilesystemError::NotADirectory,
        }
        .into());
    }
    nd.security_path_perm(Access::ReadWrite)?;
    Ok(vfs_unlink(nd.mnt.sb(), nd.dentry(), &child)?)
}

/// Move `from_path` beneath `from` to `to_path` beneath `to`. A rename
/// stays within one Mount; across two it crosses devices, as across two
/// mounted devices. A directory moves only beneath a Mount that allows it,
/// as Linux's `security_path_rename` asks before the move.
pub fn filename_renameat2(
    cred: &Arc<Cred>,
    from: &Path,
    from_path: &str,
    to: &Path,
    to_path: &str,
) -> Result<(), Error> {
    let mut source = filename_parentat(cred, from, from_path)?;
    let mut target = filename_parentat(cred, to, to_path)?;
    if !Arc::ptr_eq(from.mnt(), to.mnt()) {
        return Err(FilesystemError::CrossesDevices.into());
    }
    if source.last_type != LastType::Norm || target.last_type != LastType::Norm {
        return Err(FilesystemError::ResourceBusy.into());
    }
    from.mnt().mnt_want_write()?;
    let sb = from.mnt().sb();
    let rd = start_renaming(
        sb,
        (source.dentry(), &source.name),
        (target.dentry(), &target.name),
        None,
    )?;
    source.name = rd.old_dentry.name();
    target.name = rd.new_dentry.name();
    // Unless the source is a directory, a trailing `/` on either side is
    // refused, as `filename_renameat2` refuses it.
    let is_dir = rd.old_dentry.kind()? == FileType::Directory;
    if !is_dir && (source.trailing_slash || target.trailing_slash) {
        return Err(FilesystemError::NotADirectory.into());
    }
    source.security_path_perm(Access::ReadWrite)?;
    target.security_path_perm(Access::ReadWrite)?;
    cred.security_path_rename(from.mnt(), &rd.old_dentry, &rd.new_dentry)?;
    Ok(vfs_rename(sb, &rd)?)
}

/// Hold both directories a move between them changes exclusive, as Linux's
/// `lock_rename` does: the Filesystem's rename lock first where they
/// differ, so no other such move changes which stands beneath the other,
/// then the one above the other before it, as a removal holds the
/// directory above the one it removes first, and otherwise in a fixed
/// order.
fn lock_rename(
    sb: &SuperBlock,
    old_dir: &Arc<Dentry>,
    new_dir: &Arc<Dentry>,
) -> Result<Vec<RwSemaphoreGuard>, FilesystemError> {
    if Arc::ptr_eq(old_dir, new_dir) {
        return Ok(vec![old_dir.inode_lock()?]);
    }
    let rename = sb.rename_mutex().down_write()?;
    let old_first = new_dir.is_subdir(old_dir)
        || (!old_dir.is_subdir(new_dir) && Arc::as_ptr(old_dir) < Arc::as_ptr(new_dir));
    let (first, second) = match old_first {
        true => (old_dir, new_dir),
        false => (new_dir, old_dir),
    };
    let first = first.inode_lock()?;
    Ok(vec![rename, first, second.inode_lock()?])
}

/// Both directories of a move held, and the dentries of its two names found
/// under them, as Linux's `struct renamedata` carries a move once
/// `start_renaming` returns.
pub(crate) struct RenameData {
    pub(crate) _held: Vec<RwSemaphoreGuard>,
    pub(crate) old_parent: Arc<Dentry>,
    pub(crate) old_dentry: Arc<Dentry>,
    pub(crate) new_parent: Arc<Dentry>,
    pub(crate) new_dentry: Arc<Dentry>,
    pub(crate) flag: Option<RenameFlag>,
}

/// Hold both directories of a move and find its names beneath them, as
/// Linux's `start_renaming` does through `lock_rename`, so no other move
/// changes what either name stands for before the call acts on it. A move
/// whose source stands above its target directory is refused, and one whose
/// target stands above its source directory, which only an exchange may not
/// empty.
pub(crate) fn start_renaming(
    sb: &SuperBlock,
    (old_dir, old_name): (&Arc<Dentry>, &str),
    (new_dir, new_name): (&Arc<Dentry>, &str),
    flag: Option<RenameFlag>,
) -> Result<RenameData, FilesystemError> {
    let _held = lock_rename(sb, old_dir, new_dir)?;
    let old_dentry = lookup_slow(sb, old_dir, old_name)?.1;
    let new_dentry = lookup_slow(sb, new_dir, new_name)?.1;
    if new_dir.is_subdir(&old_dentry) {
        return Err(FilesystemError::InvalidInput);
    }
    if old_dir.is_subdir(&new_dentry) {
        return Err(match flag {
            Some(RenameFlag::Exchange) => FilesystemError::InvalidInput,
            _ => FilesystemError::DirectoryNotEmpty,
        });
    }
    Ok(RenameData {
        _held,
        old_parent: old_dir.clone(),
        old_dentry,
        new_parent: new_dir.clone(),
        new_dentry,
        flag,
    })
}

/// Hold what a move of `old` to `new` changes besides their directories, as
/// Linux's `vfs_rename` does: a directory only where it leaves its parent, or
/// is replaced, since what else names it does not change; the rest in the
/// order `lock_two_nondirectories` keeps.
fn lock_moved(
    (old, is_dir): (&Dentry, bool),
    (new, new_is_dir): (&Dentry, bool),
    same_dir: bool,
    flag: Option<RenameFlag>,
) -> Result<Vec<RwSemaphoreGuard>, FilesystemError> {
    let (source, target) = (old.inode(), new.inode());
    let lock_old_subdir = !same_dir;
    let lock_new_subdir = !same_dir || flag != Some(RenameFlag::Exchange);
    let locks = match (is_dir, new_is_dir) {
        (true, _) => [
            source.filter(|_| lock_old_subdir),
            target.filter(|_| !new_is_dir || lock_new_subdir),
        ],
        (false, true) => [target.filter(|_| lock_new_subdir), source],
        (false, false) => return lock_two_nondirectories(source, target),
    };
    locks
        .into_iter()
        .flatten()
        .map(|inode| inode.lock())
        .collect()
}

/// Hold two entries that are no directories, lowest address first and one
/// only once, as Linux's `lock_two_nondirectories`.
fn lock_two_nondirectories(
    first: Option<Arc<Inode>>,
    second: Option<Arc<Inode>>,
) -> Result<Vec<RwSemaphoreGuard>, FilesystemError> {
    let mut inodes: Vec<_> = first.into_iter().chain(second).collect();
    inodes.sort_by_key(Arc::as_ptr);
    inodes.dedup_by(|a, b| Arc::ptr_eq(a, b));
    inodes.iter().map(|inode| inode.lock()).collect()
}

/// Move what `rd` holds from its old name to its new one, as Linux's
/// `vfs_rename` does once `start_renaming` holds both directories: what moves
/// held as `lock_moved` says, and its dentry following it. What stands on each
/// side is checked first, as `vfs_rename` asks `may_delete_dentry` and
/// `may_create_dentry`, and a `NoReplace` move onto a name taken is refused, as
/// the exclusive lookup before it refuses one. A directory replaced at the new
/// name is gone for whoever holds it, and an exchange swaps the two names'
/// dentries, as `d_exchange` does.
pub(crate) fn vfs_rename(sb: &SuperBlock, rd: &RenameData) -> Result<(), FilesystemError> {
    let (old_dir, old) = (&rd.old_parent, &rd.old_dentry);
    let (new_dir, new) = (&rd.new_parent, &rd.new_dentry);
    let flag = rd.flag;
    let moving = kind_at(old_dir, old)?;
    let target = kind_at(new_dir, new)?;
    if Arc::ptr_eq(old, new) || old.names_the_entry_of(new) {
        return Ok(());
    }
    let is_dir = moving == Some(FileType::Directory);
    may_delete_dentry(old_dir, moving, is_dir)?;
    match (target, flag) {
        (None, Some(RenameFlag::Exchange)) => return Err(FilesystemError::NotFound),
        (None, _) if new_dir.is_deaddir() => return Err(FilesystemError::NotFound),
        (None, _) => {}
        (Some(_), Some(RenameFlag::NoReplace)) => return Err(FilesystemError::AlreadyExists),
        (target, Some(RenameFlag::Exchange)) => {
            may_delete_dentry(new_dir, target, target == Some(FileType::Directory))?
        }
        (target, _) => may_delete_dentry(new_dir, target, is_dir)?,
    }
    let _held = lock_moved(
        (old, is_dir),
        (new, target == Some(FileType::Directory)),
        Arc::ptr_eq(old_dir, new_dir),
        flag,
    )?;
    old_dir
        .inode_ops()?
        .rename(old, new_dir.inode_ops()?.as_ref(), new, flag)?;
    let dentries = sb.dentries();
    if flag == Some(RenameFlag::Exchange) {
        dentries.exchange(old, new);
        return Ok(());
    }
    if target == Some(FileType::Directory) {
        new.mark_dead();
    }
    dentries.unhash(new);
    let (old_name, new_name) = (old.name(), new.name());
    dentries.move_dentry(old_dir, &old_name, new_dir, &new_name);
    Ok(())
}

/// Remove the empty directory `victim` in `dir`, as Linux's `vfs_rmdir` does
/// once its caller holds `dir`: the directory removed is held too, so nothing
/// is made in it meanwhile, and its dentry dies before either is let go.
pub(crate) fn vfs_rmdir(
    sb: &SuperBlock,
    dir: &Dentry,
    victim: &Arc<Dentry>,
) -> Result<(), FilesystemError> {
    may_delete_dentry(dir, kind_at(dir, victim)?, true)?;
    let _victim = victim.inode_lock()?;
    dir.inode_ops()?.rmdir(victim)?;
    victim.mark_dead();
    sb.dentries().unhash(victim);
    Ok(())
}

/// Remove the file `victim` in `dir`, as Linux's `vfs_unlink` does once its
/// caller holds `dir`, the name itself held too. Its dentry is then let go of,
/// as `d_delete` unhashes one still in use, so a file made at the name later
/// has its own.
pub(crate) fn vfs_unlink(
    sb: &SuperBlock,
    dir: &Dentry,
    victim: &Arc<Dentry>,
) -> Result<(), FilesystemError> {
    may_delete_dentry(dir, kind_at(dir, victim)?, false)?;
    // A name the directory stores under another spelling records nothing
    // here to hold.
    let _victim = victim.inode().map(|target| target.lock()).transpose()?;
    dir.inode_ops()?.unlink(victim)?;
    sb.dentries().unhash(victim);
    Ok(())
}

/// Make the directory `child` in `dir`, as Linux's `vfs_mkdir` does once its
/// caller holds `dir`.
pub(crate) fn vfs_mkdir(dir: &Dentry, child: &Dentry) -> Result<(), FilesystemError> {
    may_create_dentry(dir, child)?;
    dir.inode_ops()?.mkdir(child)
}

/// Make the file `child` in `dir` and open it as `options` asks, as Linux's
/// `atomic_open` does once its caller holds `dir`.
pub(crate) fn atomic_open(
    dir: &Dentry,
    child: &Dentry,
    options: &OpenOptions,
) -> Result<Box<dyn FileOperations>, FilesystemError> {
    may_create_dentry(dir, child)?;
    let file = dir.inode_ops()?.atomic_open(child, options)?;
    // What was made stands at the name now, as `->atomic_open` calls
    // `d_instantiate`, for a Filesystem that did not record it itself.
    child.lookup_inode(dir)?;
    Ok(file)
}

/// Give what `old` names another name, `child` in `dir`, as Linux's `vfs_link`
/// does once its caller holds `dir`, refusing a directory.
fn vfs_link(old: &Dentry, dir: &Dentry, child: &Dentry) -> Result<(), FilesystemError> {
    may_create_dentry(dir, child)?;
    if old.kind() == Ok(FileType::Directory) {
        return Err(FilesystemError::NotPermitted);
    }
    dir.inode_ops()?.link(old, child)
}

/// Make the link `child` in `dir`, as Linux's `vfs_symlink` does once its caller
/// holds `dir`.
fn vfs_symlink(dir: &Dentry, child: &Dentry, target: &str) -> Result<(), FilesystemError> {
    may_create_dentry(dir, child)?;
    dir.inode_ops()?.symlink(child, target)
}

/// What stands at `dentry` in `dir`, a link reported as itself, or `None` where
/// nothing does, as a dentry is negative. It reads the kind the dentry records,
/// as Linux's `d_is_*` do.
fn kind_at(dir: &Dentry, dentry: &Dentry) -> Result<Option<FileType>, FilesystemError> {
    dentry.lookup_inode(dir)?;
    Ok(dentry.inode().map(|recorded| recorded.kind()))
}

/// Refuse to make an entry at `child` in `dir` where one stands, or once `dir`
/// was removed, as Linux's `may_create_dentry` refuses.
pub(crate) fn may_create_dentry(dir: &Dentry, child: &Dentry) -> Result<(), FilesystemError> {
    if kind_at(dir, child)?.is_some() {
        return Err(FilesystemError::AlreadyExists);
    }
    if dir.is_deaddir() {
        return Err(FilesystemError::NotFound);
    }
    Ok(())
}

/// Refuse to remove `victim`, what stands at a name in `dir`, where
/// nothing does, where it is not the kind `is_dir` asks for, or once `dir`
/// was removed, as Linux's `may_delete_dentry` refuses.
fn may_delete_dentry(
    dir: &Dentry,
    victim: Option<FileType>,
    is_dir: bool,
) -> Result<(), FilesystemError> {
    let Some(victim) = victim else {
        return Err(FilesystemError::NotFound);
    };
    match (is_dir, victim == FileType::Directory) {
        (true, false) => return Err(FilesystemError::NotADirectory),
        (false, true) => return Err(FilesystemError::IsADirectory),
        _ => {}
    }
    if dir.is_deaddir() {
        return Err(FilesystemError::NotFound);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex, OnceLock};
    use std::thread;
    use std::time::Duration;

    use crate::filesystem::wrap::{self, Wrap};
    use crate::filesystem::{
        DentryOperations, DirEntry, FileOperations, Filesystem, FilesystemError, InodeOperations,
        Lookup, Metadata, OpenOptions, Tmpfs,
    };
    use crate::mount::{Access, Mount, Policy};
    use crate::sched::testing::count_waits;
    use crate::vfs::kern_path::{kern_path, KernPath};
    use crate::vfs::{self, Dentry, Inode, OpenHow, Path, RenameFlag};

    type Hook = Box<dyn Fn() -> Result<(), FilesystemError> + Send + Sync>;

    /// A Tmpfs root a test steps into: after it removes a directory, in
    /// place of making one, as it moves an entry, and, once `stale` is set,
    /// answering each name it holds as changed outside the runtime. A
    /// directory it opens is the Tmpfs's own, so only calls on the root
    /// are stepped into.
    #[derive(Default)]
    struct HookedTmpfs {
        tmpfs: Tmpfs,
        removed: Option<Hook>,
        making: Option<Hook>,
        renaming: Option<Hook>,
        renames: AtomicUsize,
        stale: Arc<AtomicBool>,
    }

    /// Every held dentry stale once its flag is set, and valid until then.
    struct HookedDentry(Arc<AtomicBool>);

    impl DentryOperations for HookedDentry {
        fn revalidate(
            &self,
            _dir: &dyn InodeOperations,
            _dentry: &Dentry,
        ) -> Result<bool, FilesystemError> {
            Ok(!self.0.load(Ordering::SeqCst))
        }
    }

    impl InodeOperations for HookedTmpfs {
        fn getattr(&self, dentry: &Dentry) -> Result<Metadata, FilesystemError> {
            self.tmpfs.getattr(dentry)
        }

        fn lookup(&self, dentry: &Dentry) -> Result<Lookup, FilesystemError> {
            self.tmpfs.lookup(dentry)
        }

        fn dentry_ops(&self) -> Option<Arc<dyn DentryOperations>> {
            Some(Arc::new(HookedDentry(self.stale.clone())))
        }

        fn atomic_open(
            &self,
            dentry: &Dentry,
            options: &OpenOptions,
        ) -> Result<Box<dyn FileOperations>, FilesystemError> {
            self.tmpfs.atomic_open(dentry, options)
        }

        fn open(
            &self,
            dentry: &Dentry,
            options: &OpenOptions,
        ) -> Result<Box<dyn FileOperations>, FilesystemError> {
            self.tmpfs.open(dentry, options)
        }

        fn mkdir(&self, dentry: &Dentry) -> Result<(), FilesystemError> {
            match &self.making {
                Some(making) => making(),
                None => self.tmpfs.mkdir(dentry),
            }
        }

        fn rmdir(&self, dentry: &Dentry) -> Result<(), FilesystemError> {
            self.tmpfs.rmdir(dentry)?;
            self.removed.as_ref().map_or(Ok(()), |removed| removed())
        }

        fn unlink(&self, dentry: &Dentry) -> Result<(), FilesystemError> {
            self.tmpfs.unlink(dentry)
        }

        fn rename(
            &self,
            from: &Dentry,
            to_dir: &dyn InodeOperations,
            to: &Dentry,
            flag: Option<RenameFlag>,
        ) -> Result<(), FilesystemError> {
            self.renames.fetch_add(1, Ordering::SeqCst);
            if let Some(renaming) = &self.renaming {
                renaming()?;
            }
            let to_dir = (to_dir as &dyn std::any::Any)
                .downcast_ref::<Self>()
                .map_or(to_dir, |hooked| &hooked.tmpfs);
            self.tmpfs.rename(from, to_dir, to, flag)
        }
    }

    /// A Tmpfs holding only upper-case names and reaching each by any case,
    /// as a host folding case does, answering each call by the folded path
    /// from its root; `renaming` runs as a move reaches any directory.
    #[derive(Clone)]
    struct FoldsToUpper {
        root: KernPath,
        renaming: Arc<Hook>,
    }

    /// The path `dentry` stands at, folded.
    fn folded(dentry: &Dentry) -> String {
        dentry.path_raw().to_uppercase()
    }

    /// A directory of a `FoldsToUpper`, open.
    struct FoldedDir(KernPath);

    impl FileOperations for FoldedDir {
        fn iterate_shared(&mut self, dentry: &Dentry) -> Result<Vec<DirEntry>, FilesystemError> {
            match folded(dentry).as_str() {
                "" => self.0.entries(),
                at => self.0.open_dir(at)?.entries(),
            }
        }
    }

    impl InodeOperations for FoldsToUpper {
        fn getattr(&self, dentry: &Dentry) -> Result<Metadata, FilesystemError> {
            self.root.symlink_metadata(&folded(dentry))
        }

        fn lookup(&self, dentry: &Dentry) -> Result<Lookup, FilesystemError> {
            let name = dentry.name();
            let upper = name.to_uppercase();
            match self.root.symlink_metadata(&folded(dentry)) {
                Ok(_) if upper != name => Ok(Lookup::Spelled(upper)),
                Ok(found) => Ok(Lookup::Found(Inode::new(&found, Arc::new(self.clone())))),
                Err(FilesystemError::NotFound) => Ok(Lookup::Absent),
                Err(err) => Err(err),
            }
        }

        fn open(
            &self,
            dentry: &Dentry,
            options: &OpenOptions,
        ) -> Result<Box<dyn FileOperations>, FilesystemError> {
            if dentry.kind() == Ok(crate::filesystem::FileType::Directory) {
                return Ok(Box::new(FoldedDir(self.root.clone())));
            }
            self.root.open(&folded(dentry), options)
        }

        fn atomic_open(
            &self,
            dentry: &Dentry,
            options: &OpenOptions,
        ) -> Result<Box<dyn FileOperations>, FilesystemError> {
            self.root.create(&folded(dentry), options)
        }

        fn mkdir(&self, dentry: &Dentry) -> Result<(), FilesystemError> {
            self.root.create_dir(&folded(dentry))
        }

        fn rmdir(&self, dentry: &Dentry) -> Result<(), FilesystemError> {
            self.root.remove_dir(&folded(dentry))
        }

        fn unlink(&self, dentry: &Dentry) -> Result<(), FilesystemError> {
            self.root.remove_file(&folded(dentry))
        }

        fn rename(
            &self,
            from: &Dentry,
            _to_dir: &dyn InodeOperations,
            to: &Dentry,
            _flag: Option<RenameFlag>,
        ) -> Result<(), FilesystemError> {
            (self.renaming)()?;
            self.root.rename(&folded(from), &self.root, &folded(to))
        }
    }

    /// A directory where every name is a link that would open if asked,
    /// counting how often it was.
    struct OpensItsLinks(Arc<AtomicUsize>);

    impl InodeOperations for OpensItsLinks {
        fn getattr(&self, _dentry: &Dentry) -> Result<Metadata, FilesystemError> {
            Ok(Metadata::new(crate::filesystem::FileType::Symlink, 0))
        }

        fn lookup(&self, _dentry: &Dentry) -> Result<Lookup, FilesystemError> {
            let link = Metadata::new(crate::filesystem::FileType::Symlink, 0);
            let ops = Arc::new(Self(self.0.clone()));
            Ok(Lookup::Found(Inode::new(&link, ops)))
        }

        fn open(
            &self,
            _dentry: &Dentry,
            _options: &OpenOptions,
        ) -> Result<Box<dyn FileOperations>, FilesystemError> {
            struct OpenLink;
            impl FileOperations for OpenLink {}
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(Box::new(OpenLink))
        }
    }

    // A built-in Filesystem refuses a link itself, so only one that would open
    // it shows the VFS is what holds this line for every third party.
    /// What each call answers beneath a mount `prepare` finishes, over a
    /// Tmpfs holding the directory `dir`.
    fn answers(prepare: impl Fn(Mount) -> Mount, access: Access) -> [Result<(), vfs::Error>; 3] {
        let tmpfs: Tmpfs = [("dir/file", "x")].into_iter().collect();
        let mount = prepare(
            Mount::new("/data", Filesystem::new(Arc::new(tmpfs)), access)
                .expect("/data is a guest path"),
        );
        let cred = crate::security::prepare_creds([&mount]);
        let root = Path::root(mount.mnt());
        [
            vfs::filename_mkdirat(&cred, &root, "dir"),
            vfs::filename_rmdir(&cred, &root, "missing"),
            vfs::filename_unlinkat(&cred, &root, "missing"),
        ]
    }

    struct RefusesAll;

    impl Policy for RefusesAll {
        fn allows(&self, _path: &str, _access: Access) -> bool {
            false
        }
    }

    // Linux's answers in a container: `mkdir` of what stands is EEXIST on a
    // read-only mount, `rmdir` and `unlink` of what does not are EROFS.
    // @behavior RT-205
    #[test]
    fn a_read_only_mount_answers_what_stands_in_linuxs_order() {
        assert_eq!(
            answers(|mount| mount, Access::Read),
            [
                Err(vfs::Error::Filesystem(FilesystemError::AlreadyExists)),
                Err(vfs::Error::Denied(vfs::Denial::ReadOnly)),
                Err(vfs::Error::Denied(vfs::Denial::ReadOnly)),
            ],
            "vfs::filename_mkdirat of what stands beneath a read-only Mount must answer EEXIST, \
             and vfs::filename_rmdir and filename_unlinkat of what does not EROFS"
        );
    }

    // @behavior RT-206
    #[test]
    fn a_policy_is_asked_only_once_a_call_holds_what_stands() {
        assert_eq!(
            answers(
                |mount| mount.with_policy(Arc::new(RefusesAll)),
                Access::ReadWrite
            ),
            [
                Err(vfs::Error::Filesystem(FilesystemError::AlreadyExists)),
                Err(vfs::Error::Filesystem(FilesystemError::NotFound)),
                Err(vfs::Error::Filesystem(FilesystemError::NotFound)),
            ],
            "vfs::filename_mkdirat of what stands, and vfs::filename_rmdir and filename_unlinkat \
             of what does not, beneath a Policy refusing everything, must answer as \
             security_path_* would after the name is held"
        );
    }

    // @behavior RT-207
    #[test]
    fn a_move_across_mounts_answers_ahead_of_a_dot_at_its_end() {
        let mount = |tmpfs: Tmpfs| {
            Mount::new("/data", Filesystem::new(Arc::new(tmpfs)), Access::ReadWrite)
                .expect("/data is a guest path")
        };
        let (first, second) = (mount(Tmpfs::new()), mount(Tmpfs::new()));
        assert_eq!(
            vfs::filename_renameat2(
                &Arc::default(),
                &Path::root(first.mnt()),
                ".",
                &Path::root(second.mnt()),
                "moved"
            ),
            Err(vfs::Error::Filesystem(FilesystemError::CrossesDevices)),
            "vfs::filename_renameat2 of `.` to another Mount must answer EXDEV, as \
             filename_renameat2 answers a move across mounts first"
        );
    }

    /// A directory where every name is one entry, as hard links are, counting
    /// each move it is asked for.
    struct OneEntry(Arc<AtomicUsize>);

    impl InodeOperations for OneEntry {
        fn getattr(&self, _dentry: &Dentry) -> Result<Metadata, FilesystemError> {
            Ok(Metadata::new(crate::filesystem::FileType::Directory, 0))
        }

        fn lookup(&self, _dentry: &Dentry) -> Result<Lookup, FilesystemError> {
            let entry = Metadata::new(crate::filesystem::FileType::File, 0).with_identity(1, 2);
            Ok(Lookup::Found(Inode::new(
                &entry,
                Arc::new(Self(self.0.clone())),
            )))
        }

        fn rename(
            &self,
            _from: &Dentry,
            _to_dir: &dyn InodeOperations,
            _to: &Dentry,
            _flag: Option<RenameFlag>,
        ) -> Result<(), FilesystemError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    // @behavior RT-209
    #[test]
    fn a_move_between_two_names_of_one_entry_changes_nothing() {
        let moves = Arc::new(AtomicUsize::new(0));
        let root = root_of(Filesystem::new(Arc::new(OneEntry(moves.clone()))));
        assert_eq!(
            (
                vfs::filename_renameat2(&Arc::default(), &root, "a", &root, "b"),
                moves.load(Ordering::SeqCst)
            ),
            (Ok(()), 0),
            "vfs::filename_renameat2 between two names of one entry must succeed without asking \
             the Filesystem, as vfs_rename compares source and target"
        );
    }

    // Every Invocation holds the hook refusing a guest's readlink, so only
    // what it answers around that refusal tells the order.
    // @behavior RT-208
    #[test]
    fn a_readlink_asks_its_hook_only_of_a_link() {
        let tmpfs: Tmpfs = [("file", "x")].into_iter().collect();
        let plain = Mount::new("/data", Filesystem::new(Arc::new(tmpfs)), Access::ReadWrite)
            .expect("/data is a guest path");
        let links = Mount::new(
            "/data",
            Filesystem::new(Arc::new(OpensItsLinks(Arc::default()))),
            Access::ReadWrite,
        )
        .expect("/data is a guest path");
        let cred = crate::security::prepare_creds([&plain, &links]);
        let (plain, links) = (Path::root(plain.mnt()), Path::root(links.mnt()));
        assert_eq!(
            (
                vfs::do_readlinkat(&cred, &plain, "missing"),
                vfs::do_readlinkat(&cred, &plain, "file"),
                vfs::do_readlinkat(&cred, &links, "link"),
            ),
            (
                Err(vfs::Error::Filesystem(FilesystemError::NotFound)),
                Err(vfs::Error::Filesystem(FilesystemError::InvalidInput)),
                Err(vfs::Error::Filesystem(FilesystemError::Unsupported)),
            ),
            "vfs::do_readlinkat must answer ENOENT for what does not stand, EINVAL for what is no \
             link, and only a link what the security hook answers"
        );
    }

    // @behavior RT-200
    #[test]
    fn an_open_refuses_a_link_before_its_filesystem_is_asked() {
        let asked = Arc::new(AtomicUsize::new(0));
        let root = root_of(Filesystem::new(Arc::new(OpensItsLinks(asked.clone()))));
        let answer = vfs::do_file_open(&Arc::default(), &root, "link", &OpenHow::default());
        assert_eq!(
            (answer.err(), asked.load(Ordering::SeqCst)),
            (
                Some(vfs::Error::Filesystem(FilesystemError::FilesystemLoop)),
                0
            ),
            "vfs::do_file_open of a link following none, over a Filesystem that would open it, \
             must answer ELOOP and leave the Filesystem unasked"
        );
    }

    fn root_of(filesystem: Arc<Filesystem>) -> Path {
        let mount =
            Mount::new("/data", filesystem, Access::ReadWrite).expect("/data is a guest path");
        Path::root(mount.mnt())
    }

    fn open_dir(start: &Path, path: &str) -> Path {
        let how = OpenHow {
            directory: true,
            ..OpenHow::default()
        };
        match vfs::do_file_open(&Arc::default(), start, path, &how) {
            Ok(dir) => dir.path().clone(),
            Err(_) => panic!("{path} must open as a directory"),
        }
    }

    // The removal pauses between the Filesystem removing the directory and
    // the VFS marking its dentry dead, which is where a make and an open of
    // the same name would take the dentry about to die.
    // @behavior RT-163
    // A Tmpfs whiteout is a character device 0:0, which a Tmpfs opens as
    // unsupported, so only the VFS refusing it as a device answers EACCES.
    // @behavior RT-174
    #[test]
    fn an_entry_neither_file_nor_directory_opens_as_access_denied() {
        let tmpfs = Tmpfs::new();
        tmpfs.whiteout("w").expect("the whiteout must be made");
        let root = root_of(Filesystem::new(Arc::new(tmpfs)));
        let how = OpenHow {
            read: true,
            ..OpenHow::default()
        };
        assert_eq!(
            vfs::do_file_open(&crate::security::prepare_creds([]), &root, "w", &how).err(),
            Some(vfs::Error::Filesystem(FilesystemError::PermissionDenied)),
            "a character device through vfs::do_file_open must answer PermissionDenied"
        );
    }

    // A held name its Filesystem answers as changed outside the runtime no
    // longer stands for what is at the name now, so the removal reaches a
    // dentry of its own and the directory a descriptor holds stays alive.
    #[test]
    fn a_dentry_its_filesystem_answers_stale_is_dropped_without_dying() {
        let hooked = Arc::new(HookedTmpfs {
            tmpfs: [("d/keep", "x")].into_iter().collect(),
            ..HookedTmpfs::default()
        });
        let root = root_of(Filesystem::new(hooked.clone()));
        let held = open_dir(&root, "d");
        hooked.stale.store(true, Ordering::SeqCst);
        vfs::filename_unlinkat(&Arc::default(), &root, "d/keep").expect("the file must go");
        vfs::filename_rmdir(&Arc::default(), &root, "d").expect("the directory must go");
        assert!(
            !held.dentry().is_deaddir(),
            "vfs::filename_rmdir of a name whose held dentry DentryOperations::revalidate answers stale must \
             leave that dentry alive"
        );
    }

    #[test]
    fn a_directory_made_while_another_is_removed_in_its_place_stays_alive() {
        let (removed, removing) = mpsc::channel();
        let (made, making) = mpsc::channel::<()>();
        let (removed, making) = (Mutex::new(removed), Mutex::new(making));
        let hooked = HookedTmpfs {
            removed: Some(Box::new(move || {
                removed.lock().expect("unpoisoned").send(()).ok();
                let _ = making
                    .lock()
                    .expect("unpoisoned")
                    .recv_timeout(Duration::from_millis(200));
                Ok(())
            })),
            ..HookedTmpfs::default()
        };
        let root = root_of(Filesystem::new(Arc::new(hooked)));
        vfs::filename_mkdirat(&Arc::default(), &root, "d").expect("the directory must be made");
        let remover = {
            let root = root.clone();
            thread::spawn(move || vfs::filename_rmdir(&Arc::default(), &root, "d"))
        };
        removing
            .recv()
            .expect("the removal must reach the Filesystem");
        vfs::filename_mkdirat(&Arc::default(), &root, "d")
            .expect("another directory must be made in its place");
        let dir = open_dir(&root, "d");
        made.send(()).ok();
        remover
            .join()
            .expect("the remover must not panic")
            .expect("the removal must succeed");
        assert_eq!(
            vfs::filename_mkdirat(&Arc::default(), &dir, "inner"),
            Ok(()),
            "vfs::filename_mkdirat through a directory vfs::filename_mkdirat made while vfs::filename_rmdir \
             removed the one in its place must make into the directory made"
        );
    }

    /// A Policy allowing everything that holds its first answer until the
    /// test lets it go, or a short while passes.
    struct PausesFirst {
        answered: AtomicBool,
        reached: Mutex<mpsc::Sender<()>>,
        resume: Mutex<mpsc::Receiver<()>>,
    }

    impl Policy for PausesFirst {
        fn allows(&self, _path: &str, _access: Access) -> bool {
            if !self.answered.swap(true, Ordering::SeqCst) {
                self.reached.lock().expect("unpoisoned").send(()).ok();
                let _ = self
                    .resume
                    .lock()
                    .expect("unpoisoned")
                    .recv_timeout(Duration::from_millis(200));
            }
            true
        }
    }

    // The Policy judges the open after it finds the file and before it
    // opens it, so only the open holding its directory throughout keeps
    // the removal from landing in between.
    // @behavior RT-167
    #[test]
    fn a_create_opens_the_file_it_finds_or_makes_one() {
        let (reached, reaching) = mpsc::channel();
        let (removed, resume) = mpsc::channel();
        let policy = PausesFirst {
            answered: AtomicBool::new(false),
            reached: Mutex::new(reached),
            resume: Mutex::new(resume),
        };
        let tmpfs: Tmpfs = [("f", "x")].into_iter().collect();
        let mount = Mount::new("/data", Filesystem::new(Arc::new(tmpfs)), Access::ReadWrite)
            .expect("/data is a guest path")
            .with_policy(Arc::new(policy));
        let root = Path::root(mount.mnt());
        let cred = crate::security::prepare_creds([&mount]);
        let opener = {
            let (root, cred) = (root.clone(), cred.clone());
            thread::spawn(move || {
                let how = OpenHow {
                    write: true,
                    create: true,
                    ..OpenHow::default()
                };
                vfs::do_file_open(&cred, &root, "f", &how).map(drop)
            })
        };
        reaching.recv().expect("the open must reach the Policy");
        let remover = thread::spawn(move || {
            let answer = vfs::filename_unlinkat(&cred, &root, "f");
            removed.send(()).ok();
            answer
        });
        let opened = opener.join().expect("the open must not panic");
        remover
            .join()
            .expect("the removal must not panic")
            .expect("the removal must land");
        assert_eq!(
            opened,
            Ok(()),
            "vfs::do_file_open with create of a file vfs::filename_unlinkat removes while the open is judged \
             must open it or make it, never answer NotFound"
        );
    }

    // The Filesystem answers a move as its Tmpfs would, so only a count of
    // zero shows the VFS refused it before asking.
    // @behavior RT-164
    #[test]
    fn a_directory_moved_beneath_itself_is_refused_whatever_its_filesystem() {
        let hooked = Arc::new(HookedTmpfs::default());
        let root = root_of(Filesystem::new(hooked.clone()));
        vfs::filename_mkdirat(&Arc::default(), &root, "a").expect("the directory must be made");
        vfs::filename_mkdirat(&Arc::default(), &root, "a/b")
            .expect("the directory beneath it must be made");
        assert_eq!(
            (
                vfs::filename_renameat2(&Arc::default(), &root, "a", &root, "a/b/a"),
                hooked.renames.load(Ordering::SeqCst)
            ),
            (
                Err(vfs::Error::Filesystem(FilesystemError::InvalidInput)),
                0
            ),
            "vfs::filename_renameat2 of a directory into a directory beneath it must answer InvalidInput \
             without asking the Filesystem"
        );
    }

    // @behavior RT-165
    #[test]
    fn a_filesystem_changing_the_directory_its_own_change_holds_answers_a_deadlock() {
        let root = Arc::new(OnceLock::<Path>::new());
        let hooked = HookedTmpfs {
            making: Some(Box::new({
                let root = root.clone();
                move || match vfs::filename_mkdirat(
                    &Arc::default(),
                    root.get().expect("the root is set"),
                    "other",
                ) {
                    Err(vfs::Error::Filesystem(err)) => Err(err),
                    _ => Ok(()),
                }
            })),
            ..HookedTmpfs::default()
        };
        let path = root_of(Filesystem::new(Arc::new(hooked)));
        root.set(path.clone()).ok();
        assert_eq!(
            vfs::filename_mkdirat(&Arc::default(), &path, "made"),
            Err(vfs::Error::Filesystem(FilesystemError::Deadlock)),
            "vfs::filename_mkdirat called by a Filesystem's own create_dir on the directory the outer \
             vfs::filename_mkdirat holds must answer Deadlock"
        );
    }

    // The make starts from a descriptor already standing in `b`, so only the
    // move holding `b` itself, not the directory above it, keeps the make out.
    // Landing there would leave `b` not empty and refuse the move.
    // @behavior RT-179
    #[test]
    fn a_directory_a_move_replaces_makes_nothing_more() {
        let counts = count_waits();
        let (renamed, renaming) = mpsc::channel();
        let renamed = Mutex::new(renamed);
        let hooked = HookedTmpfs {
            tmpfs: [("a/keep", "x")].into_iter().collect(),
            renaming: Some(Box::new({
                let counts = counts.clone();
                move || {
                    renamed.lock().expect("unpoisoned").send(()).ok();
                    // Bounded, so a make that never waits fails the assertion
                    // rather than hanging the test.
                    for _ in 0..2000 {
                        if counts.load(Ordering::SeqCst) > 0 {
                            break;
                        }
                        thread::sleep(Duration::from_millis(1));
                    }
                    Ok(())
                }
            })),
            ..HookedTmpfs::default()
        };
        let root = root_of(Filesystem::new(Arc::new(hooked)));
        vfs::filename_mkdirat(&Arc::default(), &root, "b").expect("the directory must be made");
        let replaced = open_dir(&root, "b");
        let mover = {
            let root = root.clone();
            thread::spawn(move || vfs::filename_renameat2(&Arc::default(), &root, "a", &root, "b"))
        };
        renaming.recv().expect("the move must reach the Filesystem");
        let made = vfs::filename_mkdirat(&Arc::default(), &replaced, "made");
        let moved = mover.join().expect("the mover must not panic");
        assert_eq!(
            (made, moved),
            (
                Err(vfs::Error::Filesystem(FilesystemError::NotFound)),
                Ok(())
            ),
            "vfs::filename_mkdirat through a directory vfs::filename_renameat2 is replacing must \
             wait for the move, answer NotFound, and leave the move to succeed"
        );
    }

    // @behavior RT-166
    #[test]
    fn a_call_waiting_for_a_directorys_lock_waits_through_the_registered_blocking() {
        let counts = count_waits();
        let (removed, removing) = mpsc::channel();
        let removed = Mutex::new(removed);
        let hooked = HookedTmpfs {
            removed: Some(Box::new({
                let counts = counts.clone();
                move || {
                    removed.lock().expect("unpoisoned").send(()).ok();
                    // Bounded, so a call that never waits fails the count
                    // rather than hanging the test.
                    for _ in 0..2000 {
                        if counts.load(Ordering::SeqCst) > 0 {
                            break;
                        }
                        thread::sleep(Duration::from_millis(1));
                    }
                    Ok(())
                }
            })),
            ..HookedTmpfs::default()
        };
        let root = root_of(Filesystem::new(Arc::new(hooked)));
        vfs::filename_mkdirat(&Arc::default(), &root, "d").expect("the directory must be made");
        let remover = {
            let root = root.clone();
            thread::spawn(move || vfs::filename_rmdir(&Arc::default(), &root, "d"))
        };
        removing
            .recv()
            .expect("the removal must reach the Filesystem");
        let made = vfs::filename_mkdirat(&Arc::default(), &root, "e");
        remover
            .join()
            .expect("the remover must not panic")
            .expect("the removal must succeed");
        assert_eq!(
            (made, counts.load(Ordering::SeqCst)),
            (Ok(()), 1),
            "vfs::filename_mkdirat on a directory another thread's vfs::filename_rmdir holds must wait \
             through the Blocking given to sched::register_blocking, then answer"
        );
    }

    // Each spelling of `DIR` kept apart would be a dentry and a lock of its
    // own, so only the two sharing one makes the removal wait for the move.
    // @behavior RT-169
    #[test]
    fn every_spelling_of_a_name_holds_its_one_dentry_and_lock() {
        let counts = count_waits();
        let (reached, reaching) = mpsc::channel();
        let renaming: Arc<Hook> = Arc::new(Box::new({
            let (counts, reached) = (counts.clone(), Mutex::new(reached));
            move || {
                reached.lock().expect("unpoisoned").send(()).ok();
                // Bounded, so a removal that never waits fails the count
                // rather than hanging the test.
                for _ in 0..2000 {
                    if counts.load(Ordering::SeqCst) > 0 {
                        break;
                    }
                    thread::sleep(Duration::from_millis(1));
                }
                Ok(())
            }
        }));
        let tmpfs: Tmpfs = [("DIR/F", "x")].into_iter().collect();
        let folds = FoldsToUpper {
            root: kern_path(Arc::new(tmpfs)),
            renaming,
        };
        let root = root_of(Filesystem::new(Arc::new(folds)));
        let mover = {
            let root = root.clone();
            thread::spawn(move || {
                vfs::filename_renameat2(&Arc::default(), &root, "dir/f", &root, "dir/g")
            })
        };
        // Bounded, so a move that never reaches the Filesystem fails the
        // assertion rather than hanging the test.
        reaching.recv_timeout(Duration::from_secs(5)).ok();
        let removed = vfs::filename_unlinkat(&Arc::default(), &root, "DIR/F");
        let moved = mover.join().expect("the mover must not panic");
        assert_eq!(
            (moved, removed, counts.load(Ordering::SeqCst) > 0),
            (
                Ok(()),
                Err(vfs::Error::Filesystem(FilesystemError::NotFound)),
                true
            ),
            "vfs::filename_unlinkat of `DIR/F` while vfs::filename_renameat2 moves `dir/f`, over a Filesystem \
             storing both as `DIR/F`, must wait for the move and then find nothing"
        );
    }

    // Called with no Driver in front, so the VFS is what answers for every ABI.
    // @behavior RT-213
    #[test]
    fn a_path_no_call_can_take_is_refused_before_a_filesystem_is_asked() {
        let mount = Mount::new(
            "/data",
            Filesystem::new(Arc::new(Tmpfs::new())),
            Access::ReadWrite,
        )
        .expect("/data is a guest path");
        let root = Path::root(mount.mnt());
        let cred = Arc::default();
        let error = |error| Err(vfs::Error::Filesystem(error));
        assert_eq!(
            [
                vfs::filename_mkdirat(&cred, &root, "a\0b"),
                vfs::filename_mkdirat(&cred, &root, &"a/".repeat(2048)),
                vfs::filename_mkdirat(&cred, &root, ""),
                vfs::filename_symlinkat(&cred, "", &root, "link"),
                vfs::filename_symlinkat(&cred, "a\0b", &root, "link"),
            ],
            [
                error(FilesystemError::InvalidInput),
                error(FilesystemError::InvalidFilename),
                error(FilesystemError::NotFound),
                error(FilesystemError::NotFound),
                error(FilesystemError::InvalidInput),
            ],
            "vfs::filename_mkdirat and filename_symlinkat must answer EINVAL for a NUL, \
             ENAMETOOLONG for a path that fits no PATH_MAX, and ENOENT for an empty path or target"
        );
    }

    /// A directory holding `file`, counting each lookup of it, whose names go
    /// once nothing holds them where `deleted` says.
    #[derive(Clone)]
    struct CountsLookups {
        looked_up: Arc<AtomicUsize>,
        deleted: bool,
    }

    impl DentryOperations for CountsLookups {
        fn revalidate(
            &self,
            _dir: &dyn InodeOperations,
            _dentry: &Dentry,
        ) -> Result<bool, FilesystemError> {
            Ok(true)
        }

        fn delete(&self, _dentry: &Dentry) -> bool {
            self.deleted
        }
    }

    impl InodeOperations for CountsLookups {
        fn getattr(&self, dentry: &Dentry) -> Result<Metadata, FilesystemError> {
            let kind = match dentry.parent() {
                Some(_) => crate::filesystem::FileType::File,
                None => crate::filesystem::FileType::Directory,
            };
            Ok(Metadata::new(kind, 0))
        }

        fn lookup(&self, _dentry: &Dentry) -> Result<Lookup, FilesystemError> {
            self.looked_up.fetch_add(1, Ordering::SeqCst);
            let file = Metadata::new(crate::filesystem::FileType::File, 0);
            Ok(Lookup::Found(Inode::new(&file, Arc::new(self.clone()))))
        }

        fn dentry_ops(&self) -> Option<Arc<dyn DentryOperations>> {
            Some(Arc::new(self.clone()))
        }
    }

    /// How many lookups two stats of one name, with nothing held between, ask
    /// of a Filesystem answering `deleted`.
    fn lookups_of_two_stats(deleted: bool) -> usize {
        let looked_up = Arc::new(AtomicUsize::new(0));
        let root = root_of(Filesystem::new(Arc::new(CountsLookups {
            looked_up: looked_up.clone(),
            deleted,
        })));
        for _ in 0..2 {
            vfs::vfs_fstatat(&Arc::default(), &root, "file", false)
                .unwrap_or_else(|_| panic!("the file must stand"));
        }
        looked_up.load(Ordering::SeqCst)
    }

    // @behavior RT-216
    #[test]
    fn a_name_found_standing_stays_unless_its_filesystem_lets_it_go() {
        assert_eq!(
            (lookups_of_two_stats(false), lookups_of_two_stats(true)),
            (1, 2),
            "two vfs::vfs_fstatat of one name with nothing held between must look it up once, \
             and twice where DentryOperations::delete lets it go"
        );
    }

    /// A directory holding one file at every name, recording each call.
    #[derive(Clone, Default)]
    struct RecordsCalls(Arc<Mutex<Vec<&'static str>>>);

    /// A file answering no call but those it records.
    struct RecordedFile;
    impl FileOperations for RecordedFile {}

    impl InodeOperations for RecordsCalls {
        fn getattr(&self, _dentry: &Dentry) -> Result<Metadata, FilesystemError> {
            Ok(Metadata::new(crate::filesystem::FileType::Directory, 0))
        }

        fn lookup(&self, dentry: &Dentry) -> Result<Lookup, FilesystemError> {
            if dentry.name() == "missing" {
                return Ok(Lookup::Absent);
            }
            let file = Metadata::new(crate::filesystem::FileType::File, 0);
            Ok(Lookup::Found(Inode::new(&file, Arc::new(self.clone()))))
        }

        fn setattr(
            &self,
            _dentry: &Dentry,
            attr: &crate::filesystem::Iattr,
        ) -> Result<(), FilesystemError> {
            let call = if attr.open {
                "setattr(open)"
            } else {
                "setattr"
            };
            self.0.lock().expect("never poisoned").push(call);
            Ok(())
        }

        fn open(
            &self,
            _dentry: &Dentry,
            _options: &OpenOptions,
        ) -> Result<Box<dyn FileOperations>, FilesystemError> {
            self.0.lock().expect("never poisoned").push("open");
            Ok(Box::new(RecordedFile))
        }

        fn atomic_open(
            &self,
            _dentry: &Dentry,
            _options: &OpenOptions,
        ) -> Result<Box<dyn FileOperations>, FilesystemError> {
            self.0.lock().expect("never poisoned").push("atomic_open");
            Ok(Box::new(RecordedFile))
        }
    }

    // @behavior RT-215
    #[test]
    fn an_open_empties_what_stood_through_setattr_once_it_is_open() {
        let calls = RecordsCalls::default();
        let root = root_of(Filesystem::new(Arc::new(calls.clone())));
        let cred = Arc::default();
        let truncate = OpenHow {
            write: true,
            create: true,
            truncate: true,
            ..OpenHow::default()
        };
        let opened = (
            vfs::do_file_open(&cred, &root, "file", &truncate).is_ok(),
            vfs::do_file_open(&cred, &root, "missing", &truncate).is_ok(),
        );
        assert_eq!(
            (opened, calls.0.lock().map(|calls| calls.clone()).ok()),
            (
                (true, true),
                Some(vec!["open", "setattr(open)", "atomic_open"])
            ),
            "vfs::do_file_open with O_TRUNC of a file that stands must open it and then ask \
             setattr marked as from an open, and of one it makes must ask neither"
        );
    }

    /// Holds a move of `k` inside its Filesystem, its directories and the
    /// rename lock held, until told to go on.
    struct HoldsMoveOfK {
        reached: Mutex<mpsc::Sender<()>>,
        go_on: Mutex<mpsc::Receiver<()>>,
    }

    impl wrap::Hook for HoldsMoveOfK {
        fn rename(
            &self,
            inner: &dyn InodeOperations,
            from: &Dentry,
            to_dir: &dyn InodeOperations,
            to: &Dentry,
            flag: Option<RenameFlag>,
        ) -> Result<(), FilesystemError> {
            if from.name() == "k" {
                let reached = self.reached.lock().expect("never poisoned");
                reached.send(()).expect("the test waits to hear");
                let go_on = self.go_on.lock().expect("never poisoned");
                go_on.recv().expect("the test says when to go on");
            }
            inner.rename(from, to_dir, to, flag)
        }
    }

    /// Refuses every change to the one path the move may not touch.
    struct RefusesMovedName;

    impl Policy for RefusesMovedName {
        fn allows(&self, path: &str, _access: Access) -> bool {
            path != "/data/a/moved"
        }
    }

    // The move waits for the rename lock another move holds, having walked
    // to `a/x`. A writer with no Policy renames it within `a` meanwhile, which
    // takes no rename lock. A dentry taken before the wait would follow it to
    // the name the Policy refuses, so only a name found under the lock answers
    // ENOENT. The wait can only be slept for; a move not yet waiting is one the
    // assertion passes on, never one it fails.
    // @behavior RT-211
    #[test]
    fn a_move_acts_on_what_stands_at_its_names_once_it_holds_them() {
        let tmpfs: Tmpfs = [("a/x", "X"), ("b/n", "n"), ("c/k", "k"), ("d/n", "n")]
            .into_iter()
            .collect();
        let (reached, reaching) = mpsc::channel();
        let (go_on, going_on) = mpsc::channel();
        let hook = Arc::new(HoldsMoveOfK {
            reached: Mutex::new(reached),
            go_on: Mutex::new(going_on),
        });
        let mount = Mount::new(
            "/data",
            Filesystem::new(Arc::new(Wrap::new(Arc::new(tmpfs), hook))),
            Access::ReadWrite,
        )
        .expect("/data is a guest path")
        .with_policy(Arc::new(RefusesMovedName));
        let cred = crate::security::prepare_creds([&mount]);
        let root = Path::root(mount.mnt());
        let holder = {
            let root = root.clone();
            thread::spawn(move || {
                vfs::filename_renameat2(&Arc::default(), &root, "c/k", &root, "d/k")
            })
        };
        reaching
            .recv_timeout(Duration::from_secs(5))
            .expect("the held move must reach its Filesystem");
        let mover = {
            let (cred, root) = (cred.clone(), root.clone());
            thread::spawn(move || vfs::filename_renameat2(&cred, &root, "a/x", &root, "b/y"))
        };
        thread::sleep(Duration::from_millis(200));
        let renamed = vfs::filename_renameat2(&Arc::default(), &root, "a/x", &root, "a/moved");
        go_on.send(()).expect("the held move must be waiting");
        holder
            .join()
            .expect("the holder must not panic")
            .unwrap_or_else(|_| panic!("the held move must land"));
        let moved = mover.join().expect("the mover must not panic");
        let stands = |path| vfs::vfs_fstatat(&cred, &root, path, false).is_ok();
        assert_eq!(
            (renamed, moved, stands("a/moved"), stands("b/y")),
            (
                Ok(()),
                Err(vfs::Error::Filesystem(FilesystemError::NotFound)),
                true,
                false
            ),
            "vfs::filename_renameat2 of `a/x` that waited while another writer renamed it must \
             answer ENOENT and leave `a/moved`, which the Policy refuses, where it stands"
        );
    }
}
