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
use super::openat2::{build_open_flags, do_truncate, OpenHow, OpenOptions};
use super::path::Path;
use super::super_block::SuperBlock;
use super::{FileType, FilesystemError, Iattr, Metadata};
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
/// it stands in, the names still to walk, the last name left for the call to
/// act on, and what the walk ended on once it looked.
pub(crate) struct Nameidata {
    pub(crate) mnt: Arc<VfsMount>,
    pub(crate) cred: Arc<Cred>,
    /// Each directory from the one the walk started from, which no `..`
    /// climbs above, down to the one it stands in.
    frames: Vec<Arc<Dentry>>,
    pending: VecDeque<String>,
    links: usize,
    /// The last name, as Linux's `nd->last`, empty unless it is a `Norm` one.
    pub(crate) name: String,
    last_type: LastType,
    /// Whether a `/` follows the last name, as `nd->last.name[nd->last.len]`.
    trailing_slash: bool,
    /// The dentry the walk ended on, as `nd->path` holds it, naming nothing
    /// where nothing stands at the last name, so what the lookup recorded there
    /// is what the call acts on.
    path: Option<Arc<Dentry>>,
    /// Whether only a directory satisfies the path, as `LOOKUP_DIRECTORY`,
    /// which stays once a trailing `/` sets it, through any link it follows.
    lookup_directory: bool,
}

impl Nameidata {
    /// The directory the walk stands in, which holds the last name.
    pub(crate) fn dentry(&self) -> &Arc<Dentry> {
        self.frames.last().expect("a walk stands somewhere")
    }

    /// The dentry the walk ended on, as Linux's `do_open` and `vfs_getattr`
    /// act on what the walk found without looking it up again, so a change
    /// made since cannot lead the call anywhere but where the walk stands.
    pub(crate) fn path(&self) -> Result<Arc<Dentry>, FilesystemError> {
        self.path.clone().ok_or(FilesystemError::NotFound)
    }

    /// Find the last name afresh and keep what stands there, as a call looks
    /// its name up once it holds the directory's lock.
    fn lookup_last_locked(&mut self) -> Result<Arc<Dentry>, FilesystemError> {
        let (stored, child) = lookup_one_qstr_excl(self.mnt.sb(), self.dentry(), &self.name)?;
        self.name = stored;
        self.path = Some(child.clone());
        Ok(child)
    }

    /// What kind of entry the walk ended on, a link reported as itself, read
    /// from the dentry as Linux's `d_is_*` do.
    pub(crate) fn kind(&self) -> Result<FileType, FilesystemError> {
        self.path()?.kind()
    }

    /// What the walk ended on reports of itself, a link reported as itself, as
    /// `vfs_getattr` asks the entry the walk found.
    pub(crate) fn getattr(&self) -> Result<Metadata, FilesystemError> {
        self.path()?.getattr()
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
    fn check(&self, access: Access) -> Result<(), FilesystemError> {
        if access == Access::ReadWrite {
            self.mnt.mnt_want_write()?;
        }
        self.security_path_perm(access)
    }

    /// Whether a security hook lets the path be reached with `access`, as
    /// Linux's `security_path_*` asks once its call holds the name.
    fn security_path_perm(&self, access: Access) -> Result<(), FilesystemError> {
        self.cred
            .security_path_perm(&self.mnt, &|| self.dentry_path(), access)
    }
}

/// The dentry held for `name` in `parent`, as Linux's `lookup_dcache` finds one:
/// it is asked whether it still names what stands there, as `d_revalidate`
/// answers, and let go of where it does not.
fn lookup_dcache(
    sb: &SuperBlock,
    parent: &Arc<Dentry>,
    name: &str,
) -> Result<Option<Arc<Dentry>>, FilesystemError> {
    let dentries = sb.dentries();
    let Some(held) = dentries.lookup(parent, name) else {
        return Ok(None);
    };
    Ok(d_revalidate(sb, parent, &held)?.then_some(held))
}

/// Whether `dentry` still names what stands at its name in `dir`, whose lock the
/// caller holds, so no move lands between the question and the answer. One that
/// does not is let go of, as `d_invalidate` unhashes it.
fn d_revalidate(
    sb: &SuperBlock,
    dir: &Dentry,
    dentry: &Arc<Dentry>,
) -> Result<bool, FilesystemError> {
    let valid = match dentry.ops() {
        Some(ops) => ops.revalidate(dir.inode_ops()?.as_ref(), dentry)?,
        None => true,
    };
    if !valid {
        sb.dentries().unhash(dentry);
    }
    Ok(valid)
}

/// Whether each name from the root down to `path` still names what the runtime
/// recorded, as gVisor's `revalidatePath` walks one. Linux asks only names a walk
/// passes through, so this is kobako's own, for a security module to ask of where
/// a call starts. A name moved while it was asked is asked again from the root,
/// where it went.
pub(crate) fn revalidate_path(path: &Path) -> Result<bool, FilesystemError> {
    'again: loop {
        let mut chain = vec![path.dentry().clone()];
        while let Some(above) = chain.last().and_then(|at| at.parent()) {
            chain.push(above);
        }
        for pair in chain.windows(2).rev() {
            let [name, above] = pair else { continue };
            let _held = above.inode_lock_shared()?;
            if !name.parent().is_some_and(|now| Arc::ptr_eq(&now, above)) {
                continue 'again;
            }
            if !d_revalidate(path.mnt().sb(), above, name)? {
                return Ok(false);
            }
        }
        return Ok(true);
    }
}

/// `name` in `parent` as that directory stores it, and the dentry of that
/// spelling with what stands there recorded on it, as Linux's
/// `lookup_one_qstr_excl` asks `->lookup` and a case-insensitive filesystem
/// answers through `d_add_ci`: every spelling reaching one entry shares its
/// dentry, and so its lock. The caller holds that directory's lock. A directory
/// removed through the VFS is asked about no name, as Linux refuses a dead one.
pub(crate) fn lookup_one_qstr_excl(
    sb: &SuperBlock,
    parent: &Arc<Dentry>,
    name: &str,
) -> Result<(String, Arc<Dentry>), FilesystemError> {
    let dentry_of = |name: &str| -> Result<Arc<Dentry>, FilesystemError> {
        match lookup_dcache(sb, parent, name)? {
            Some(held) => Ok(held),
            None => Ok(sb.dentries().child(parent, name)),
        }
    };
    let given = dentry_of(name)?;
    if given.inode().is_none() && parent.is_deaddir() {
        return Err(FilesystemError::NotFound);
    }
    let (stored, dentry) = match given.lookup_inode(parent)? {
        None => (name.to_string(), given),
        Some(stored) => {
            let dentry = dentry_of(&stored)?;
            dentry.lookup_inode(parent)?;
            (stored, dentry)
        }
    };
    sb.dentries().retain(&dentry);
    Ok((stored, dentry))
}

/// The dentry of `name` in `parent` for a walk passing through, holding that
/// directory's lock shared only to find it, as Linux's `lookup_slow` does.
fn lookup_slow(
    sb: &SuperBlock,
    parent: &Arc<Dentry>,
    name: &str,
) -> Result<Arc<Dentry>, FilesystemError> {
    let _held = parent.inode_lock_shared()?;
    Ok(lookup_one_qstr_excl(sb, parent, name)?.1)
}

/// Each name of `path`, empty between two slashes and after a last one.
fn parts(path: &str) -> VecDeque<String> {
    path.split('/').map(str::to_string).collect()
}

impl Nameidata {
    /// Step into the directory `name` names, or follow the link standing there.
    fn walk_component(&mut self, name: &str) -> Result<(), FilesystemError> {
        let dentry = lookup_slow(self.mnt.sb(), self.dentry(), name)?;
        let inode = dentry.inode().ok_or(FilesystemError::NotFound)?;
        match inode.kind() {
            FileType::Symlink => self.pick_link(&dentry),
            FileType::Directory => {
                self.frames.push(dentry);
                Ok(())
            }
            _ => Err(FilesystemError::NotADirectory),
        }
    }

    /// Walk on through where the link `dentry` names points, before
    /// whatever is left of the path.
    fn pick_link(&mut self, dentry: &Dentry) -> Result<(), FilesystemError> {
        self.cred.security_inode_follow_link(&self.mnt)?;
        self.links += 1;
        if self.links > MAXSYMLINKS {
            return Err(FilesystemError::FilesystemLoop);
        }
        let target = dentry.get_link()?;
        if target.starts_with('/') {
            return Err(FilesystemError::Escape);
        }
        if target.is_empty() {
            return Err(FilesystemError::NotFound);
        }
        for part in parts(&target).into_iter().rev() {
            self.pending.push_front(part);
        }
        Ok(())
    }

    /// Step out of the directory the walk stands in. Out of the one the
    /// walk started from, no `..` holds a capability, as `RESOLVE_BENEATH`
    /// refuses a caller's and a link's alike.
    fn follow_dotdot(&mut self) -> Result<(), FilesystemError> {
        if self.frames.len() == 1 {
            return Err(FilesystemError::Escape);
        }
        self.frames.pop();
        Ok(())
    }

    /// Walk every name but the last, which is left for the call to act on, as
    /// Linux's `link_path_walk` leaves `nd->last` and `nd->last_type`.
    fn link_path_walk(&mut self) -> Result<(), FilesystemError> {
        loop {
            let part = self.pending.pop_front().ok_or(FilesystemError::NotFound)?;
            let slashes = self
                .pending
                .iter()
                .take_while(|rest| rest.is_empty())
                .count();
            self.pending.drain(..slashes);
            if self.pending.is_empty() {
                self.set_last(&part, slashes > 0);
                return Ok(());
            }
            match part.as_str() {
                "." => {}
                ".." => self.follow_dotdot()?,
                name => self.walk_component(name)?,
            }
        }
    }

    /// Leave `name` as the last name of the walk, with what kind it is.
    fn set_last(&mut self, name: &str, trailing_slash: bool) {
        self.last_type = match name {
            "." => LastType::Dot,
            ".." => LastType::DotDot,
            _ => LastType::Norm,
        };
        self.name = match self.last_type {
            LastType::Norm => name.to_string(),
            _ => String::new(),
        };
        self.trailing_slash = trailing_slash;
    }

    /// Find what the last name stands for, as Linux's `lookup_last` does: a
    /// trailing `/` follows a link there and asks for a directory, and a link
    /// followed leaves another last name to find. A call `creating` finds the
    /// name under the directory's lock it makes one under, and is handed it
    /// back, as `lookup_open` looks a name up once with the lock held; a trailing
    /// `/` refuses what it would make, as `lookup_fast_for_open` refuses it.
    fn lookup_last(
        &mut self,
        mut follow: bool,
        creating: bool,
    ) -> Result<Option<RwSemaphoreGuard>, FilesystemError> {
        loop {
            if self.last_type != LastType::Norm {
                if self.last_type == LastType::DotDot {
                    self.follow_dotdot()?;
                }
                self.path = Some(self.dentry().clone());
                return Ok(None);
            }
            if self.trailing_slash {
                if creating {
                    return Err(FilesystemError::IsADirectory);
                }
                follow = true;
                self.lookup_directory = true;
            }
            let parent = self.dentry().clone();
            let held = match creating {
                true => parent.inode_lock()?,
                false => parent.inode_lock_shared()?,
            };
            let child = self.lookup_last_locked()?;
            if !follow || child.kind() != Ok(FileType::Symlink) {
                return Ok(creating.then_some(held));
            }
            drop(held);
            self.pick_link(&child)?;
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
/// `set_nameidata` and `path_init` make one. A path no call can take answers
/// first, wherever it starts; an absolute one is not the caller's to reach, and
/// a walk starts from nothing but a directory.
fn set_nameidata(cred: &Arc<Cred>, start: &Path, path: &str) -> Result<Nameidata, FilesystemError> {
    getname(path)?;
    if path.starts_with('/') {
        return Err(FilesystemError::Escape);
    }
    if start.dentry().kind()? != FileType::Directory {
        return Err(FilesystemError::NotADirectory);
    }
    cred.security_path_walk(start)?;
    Ok(Nameidata {
        mnt: start.mnt().clone(),
        cred: cred.clone(),
        frames: vec![start.dentry().clone()],
        pending: parts(path),
        links: 0,
        name: String::new(),
        last_type: LastType::Dot,
        trailing_slash: false,
        path: None,
        lookup_directory: false,
    })
}

/// `path` beneath `start`, walked to the directory holding its last name, which
/// is left unlooked up for the call to act on, as Linux's `filename_parentat`.
/// A `..` above `start` is not the caller's to reach.
pub(crate) fn filename_parentat(
    cred: &Arc<Cred>,
    start: &Path,
    path: &str,
) -> Result<Nameidata, FilesystemError> {
    let mut nd = set_nameidata(cred, start, path)?;
    nd.link_path_walk()?;
    Ok(nd)
}

/// `path` beneath `start`, walked to what it names, as Linux's `filename_lookup`.
/// Where `follow` asks, or a trailing `/` does, a link at the end is followed
/// too. Nothing standing at the last name answers ENOENT, as a walk does, and
/// anything but a directory where `LOOKUP_DIRECTORY` asks for one ENOTDIR.
pub(crate) fn filename_lookup(
    cred: &Arc<Cred>,
    start: &Path,
    path: &str,
    follow: bool,
) -> Result<Nameidata, FilesystemError> {
    let mut nd = set_nameidata(cred, start, path)?;
    nd.link_path_walk()?;
    nd.lookup_last(follow, false)?;
    let kind = nd.kind()?;
    if nd.lookup_directory && kind != FileType::Directory {
        return Err(FilesystemError::NotADirectory);
    }
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
) -> Result<File, FilesystemError> {
    let follow = build_open_flags(how)?;
    let mut nd = set_nameidata(cred, start, path)?;
    nd.lookup_directory = how.directory;
    nd.link_path_walk()?;
    let held = nd.lookup_last(follow, how.create)?;
    // The walk followed every link the call asked it to, so a link at the
    // end now is one swapped in since, and is opened as itself.
    let existing = nd.path()?.inode().map(|inode| inode.kind());
    // A file being made is open before the directory's lock goes, as Linux's
    // `lookup_open` holds it, so nothing made or removed in between changes what
    // the open decided. What already stood opens without it, as `do_open` does.
    let _held = held.filter(|_| existing.is_none());
    refuse_what_stands(existing, nd.lookup_directory, how)?;
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
    let options = OpenOptions {
        read: how.read,
        write: how.write,
        truncate: how.truncate,
    };
    let child = nd.path()?;
    let f_op = match existing {
        None => atomic_open(nd.dentry(), &child, &options)?,
        Some(_) => child.open(&options)?,
    };
    // Only what stood is emptied, as Linux's `do_open` calls
    // `handle_truncate` for a regular file it did not make.
    if how.truncate && existing == Some(FileType::File) {
        do_truncate(&child, &Iattr::open_truncate())?;
    }
    Ok(File::new(start.with_dentry(child), f_op, how))
}

/// What stands at the path answers before any access is judged, as Linux's
/// `do_open` answers it on a read-only mount.
fn refuse_what_stands(
    existing: Option<FileType>,
    lookup_directory: bool,
    how: &OpenHow,
) -> Result<(), FilesystemError> {
    let refusal = match existing {
        None if !how.create => Some(FilesystemError::NotFound),
        Some(_) if how.create && how.exclusive => Some(FilesystemError::AlreadyExists),
        Some(FileType::Directory) if how.create => Some(FilesystemError::IsADirectory),
        Some(FileType::Directory) | None => None,
        Some(_) if lookup_directory => Some(FilesystemError::NotADirectory),
        Some(_) => None,
    };
    match refusal {
        Some(error) => Err(error),
        None => may_open(existing, how),
    }
}

/// Refuse to open what stands at the path as Linux's `may_open` does: a
/// link, which the walk would have followed if asked, and a directory for
/// writing.
fn may_open(existing: Option<FileType>, how: &OpenHow) -> Result<(), FilesystemError> {
    match existing {
        Some(FileType::Symlink) => Err(FilesystemError::FilesystemLoop),
        Some(FileType::Directory) if how.write || how.truncate => {
            Err(FilesystemError::IsADirectory)
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
) -> Result<(), FilesystemError> {
    let old = filename_lookup(cred, old_start, old_path, follow)?;
    let target = old.path()?;
    let mut new = filename_parentat(cred, new_start, new_path)?;
    let (_held, child) = filename_create(&mut new, false)?;
    if !Arc::ptr_eq(old_start.mnt(), new_start.mnt()) {
        return Err(FilesystemError::CrossesDevices);
    }
    old.security_path_perm(Access::ReadWrite)?;
    new.security_path_perm(Access::ReadWrite)?;
    vfs_link(&target, new.dentry(), &child)
}

/// Make the link `path` beneath `start` names, pointing at `target`, as
/// Linux's `filename_symlinkat` does.
pub fn filename_symlinkat(
    cred: &Arc<Cred>,
    target: &str,
    start: &Path,
    path: &str,
) -> Result<(), FilesystemError> {
    getname(target)?;
    let mut nd = filename_parentat(cred, start, path)?;
    let (_held, child) = filename_create(&mut nd, false)?;
    nd.security_path_perm(Access::ReadWrite)?;
    vfs_symlink(nd.dentry(), &child, target)
}

/// Hold the directory a name is made in and take the name's dentry, as
/// Linux's `filename_create` does: a `.` or `..` already stands, and what stands
/// there answers before the mount refuses a change, so a call making what
/// already stands learns so even beneath a read-only mount. A trailing `/` on
/// anything but a directory being made finds nothing there to be one.
fn filename_create(
    nd: &mut Nameidata,
    want_dir: bool,
) -> Result<(RwSemaphoreGuard, Arc<Dentry>), FilesystemError> {
    if nd.last_type != LastType::Norm {
        return Err(FilesystemError::AlreadyExists);
    }
    let write = nd.mnt.mnt_want_write();
    let held = nd.dentry().inode_lock()?;
    let child = nd.lookup_last_locked()?;
    may_create_dentry(nd.dentry(), &child)?;
    if nd.trailing_slash && !want_dir {
        return Err(FilesystemError::NotFound);
    }
    write?;
    Ok((held, child))
}

/// Hold the directory a name is removed from and take the name's dentry, as
/// Linux's `start_dirop` does before any security hook is asked.
fn start_dirop(nd: &mut Nameidata) -> Result<(RwSemaphoreGuard, Arc<Dentry>), FilesystemError> {
    let held = nd.dentry().inode_lock()?;
    let child = nd.lookup_last_locked()?;
    match child.inode() {
        Some(_) => Ok((held, child)),
        None => Err(FilesystemError::NotFound),
    }
}

/// Make the directory `path` beneath `start`. A trailing `/` asks for the
/// directory being made, so it is no refusal here, as `mkdir(2)` takes it.
pub fn filename_mkdirat(cred: &Arc<Cred>, start: &Path, path: &str) -> Result<(), FilesystemError> {
    let mut nd = filename_parentat(cred, start, path)?;
    let (_held, child) = filename_create(&mut nd, true)?;
    nd.security_path_perm(Access::ReadWrite)?;
    vfs_mkdir(nd.dentry(), &child)
}

/// Remove the empty directory `path` beneath `start`, as Linux's
/// `filename_rmdir` does.
pub fn filename_rmdir(cred: &Arc<Cred>, start: &Path, path: &str) -> Result<(), FilesystemError> {
    let mut nd = filename_parentat(cred, start, path)?;
    match nd.last_type {
        LastType::Norm => {}
        LastType::Dot => return Err(FilesystemError::InvalidInput),
        LastType::DotDot => return Err(FilesystemError::DirectoryNotEmpty),
    }
    nd.mnt.mnt_want_write()?;
    let (_held, child) = start_dirop(&mut nd)?;
    nd.security_path_perm(Access::ReadWrite)?;
    vfs_rmdir(nd.mnt.sb(), nd.dentry(), &child)
}

/// Remove the file `path` beneath `start`, as Linux's `filename_unlinkat` does.
/// It follows no link at the end, so a trailing `/` is answered by the name
/// itself once it is found: no file removes as a directory.
pub fn filename_unlinkat(
    cred: &Arc<Cred>,
    start: &Path,
    path: &str,
) -> Result<(), FilesystemError> {
    let mut nd = filename_parentat(cred, start, path)?;
    if nd.last_type != LastType::Norm {
        return Err(FilesystemError::IsADirectory);
    }
    nd.mnt.mnt_want_write()?;
    let (_held, child) = start_dirop(&mut nd)?;
    if nd.trailing_slash {
        return Err(match child.kind() {
            Ok(FileType::Directory) => FilesystemError::IsADirectory,
            _ => FilesystemError::NotADirectory,
        });
    }
    nd.security_path_perm(Access::ReadWrite)?;
    vfs_unlink(nd.mnt.sb(), nd.dentry(), &child)
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
) -> Result<(), FilesystemError> {
    let mut source = filename_parentat(cred, from, from_path)?;
    let mut target = filename_parentat(cred, to, to_path)?;
    if !Arc::ptr_eq(from.mnt(), to.mnt()) {
        return Err(FilesystemError::CrossesDevices);
    }
    if source.last_type != LastType::Norm || target.last_type != LastType::Norm {
        return Err(FilesystemError::ResourceBusy);
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
        return Err(FilesystemError::NotADirectory);
    }
    source.security_path_perm(Access::ReadWrite)?;
    target.security_path_perm(Access::ReadWrite)?;
    cred.security_path_rename(from.mnt(), &rd.old_dentry, &rd.new_dentry)?;
    vfs_rename(sb, &rd)
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
/// changes what either name stands for before the call acts on it. Nothing at
/// the source, and at the target what `flag` will not take, is refused as the
/// flags `__start_renaming` looks each name up with refuse it. So is a move
/// whose source stands above its target directory, and one whose target stands
/// above its source directory, which only an exchange may not empty.
pub(crate) fn start_renaming(
    sb: &SuperBlock,
    (old_dir, old_name): (&Arc<Dentry>, &str),
    (new_dir, new_name): (&Arc<Dentry>, &str),
    flag: Option<RenameFlag>,
) -> Result<RenameData, FilesystemError> {
    let _held = lock_rename(sb, old_dir, new_dir)?;
    let old_dentry = lookup_one_qstr_excl(sb, old_dir, old_name)?.1;
    old_dentry.kind()?;
    let new_dentry = lookup_one_qstr_excl(sb, new_dir, new_name)?.1;
    match (flag, new_dentry.inode().is_some()) {
        (Some(RenameFlag::Exchange), false) => return Err(FilesystemError::NotFound),
        (Some(RenameFlag::NoReplace), true) => return Err(FilesystemError::AlreadyExists),
        _ => {}
    }
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
/// `may_create_dentry`. A directory replaced at the new name is gone for whoever
/// holds it, and an exchange swaps the two names' dentries, as `d_exchange` does.
pub(crate) fn vfs_rename(sb: &SuperBlock, rd: &RenameData) -> Result<(), FilesystemError> {
    let (old_dir, old) = (&rd.old_parent, &rd.old_dentry);
    let (new_dir, new) = (&rd.new_parent, &rd.new_dentry);
    let flag = rd.flag;
    let moving = old.kind().ok();
    let target = new.kind().ok();
    if Arc::ptr_eq(old, new) || old.names_the_entry_of(new) {
        return Ok(());
    }
    let is_dir = moving == Some(FileType::Directory);
    may_delete_dentry(old_dir, moving, is_dir)?;
    match (target, flag) {
        (None, _) => may_create_dentry(new_dir, new)?,
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
    may_delete_dentry(dir, victim.kind().ok(), true)?;
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
    may_delete_dentry(dir, victim.kind().ok(), false)?;
    let _victim = victim.inode_lock()?;
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
/// does once its caller holds `dir`: a directory is refused, and what `old`
/// names is held, so a removal landing first leaves no name to give another.
fn vfs_link(old: &Dentry, dir: &Dentry, child: &Dentry) -> Result<(), FilesystemError> {
    may_create_dentry(dir, child)?;
    if old.kind()? == FileType::Directory {
        return Err(FilesystemError::NotPermitted);
    }
    let _held = old.inode_lock()?;
    if old.getattr()?.nlink == 0 {
        return Err(FilesystemError::NotFound);
    }
    dir.inode_ops()?.link(old, child)
}

/// Make the link `child` in `dir`, as Linux's `vfs_symlink` does once its caller
/// holds `dir`.
fn vfs_symlink(dir: &Dentry, child: &Dentry, target: &str) -> Result<(), FilesystemError> {
    may_create_dentry(dir, child)?;
    dir.inode_ops()?.symlink(child, target)
}

/// Refuse to make an entry at `child` in `dir` where its lookup found one, or
/// once `dir` was removed, as Linux's `may_create_dentry` reads `d_inode`.
pub(crate) fn may_create_dentry(dir: &Dentry, child: &Dentry) -> Result<(), FilesystemError> {
    if child.inode().is_some() {
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
        DentryOperations, DirEntry, FileOperations, FileType, Filesystem, FilesystemError,
        InodeOperations, Lookup, Metadata, OpenOptions, Tmpfs,
    };
    use crate::mount::{Access, Mount, Policy};
    use crate::sched::testing::count_waits;
    use crate::vfs::kern_path::{kern_path, KernPath};
    use crate::vfs::{self, Dentry, Inode, OpenHow, Path, RenameFlag};

    type Hook = Box<dyn Fn() -> Result<(), FilesystemError> + Send + Sync>;

    /// A test's Filesystem counting what it is asked to look up.
    #[derive(Default)]
    struct CountsAsks(AtomicUsize);

    impl wrap::Hook for CountsAsks {
        fn lookup(
            &self,
            inner: &dyn InodeOperations,
            dentry: &Dentry,
        ) -> Result<Lookup, FilesystemError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            inner.lookup(dentry)
        }
    }

    /// A root holding `file` under `names` names, and nothing else, making a
    /// link without being asked for what it makes.
    #[derive(Clone)]
    struct LinksFile {
        names: u64,
        linked: Arc<AtomicUsize>,
    }

    impl InodeOperations for LinksFile {
        fn getattr(&self, dentry: &Dentry) -> Result<Metadata, FilesystemError> {
            Ok(match dentry.parent() {
                Some(_) => Metadata::new(FileType::File, 0).with_nlink(self.names),
                None => Metadata::new(FileType::Directory, 0),
            })
        }

        fn lookup(&self, dentry: &Dentry) -> Result<Lookup, FilesystemError> {
            if dentry.name() != "file" {
                return Ok(Lookup::Absent);
            }
            let file = Metadata::new(FileType::File, 0);
            Ok(Lookup::Found(Inode::new(&file, Arc::new(self.clone()))))
        }

        fn link(&self, _old: &Dentry, _dentry: &Dentry) -> Result<(), FilesystemError> {
            self.linked.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    // A removal landing just before the link leaves the entry with no name, so
    // only the count, read holding what is linked, tells the link not to bring it
    // back, as Linux's `vfs_link` reads `i_nlink`.
    // @behavior RT-224
    #[test]
    fn a_link_to_what_has_no_name_left_answers_enoent() {
        let answers = |names| {
            let linked = Arc::new(AtomicUsize::new(0));
            let filesystem = Filesystem::new(Arc::new(LinksFile {
                names,
                linked: linked.clone(),
            }));
            let root = root_of(filesystem);
            let answer =
                vfs::filename_linkat(&Arc::default(), &root, "file", &root, "other", false);
            (answer, linked.load(Ordering::SeqCst))
        };
        assert_eq!(
            (answers(0), answers(1)),
            ((Err(FilesystemError::NotFound), 0), (Ok(()), 1)),
            "vfs::filename_linkat of an entry reporting no names must answer ENOENT and ask for no \
             link, and of one reporting a name must make it"
        );
    }

    // A name found negative stays negative for the call holding the directory,
    // so asking the Filesystem again could only cost it, as a Filesystem written
    // in Ruby pays for each ask.
    // @behavior RT-223
    #[test]
    fn a_change_asks_a_filesystem_once_for_the_name_it_makes() {
        let counts = Arc::new(CountsAsks::default());
        let filesystem =
            Filesystem::new(Arc::new(Wrap::new(Arc::new(Tmpfs::new()), counts.clone())));
        let root = root_of(filesystem);
        let cred = Arc::default();
        let how = OpenHow {
            write: true,
            create: true,
            ..OpenHow::default()
        };
        let asked = |call: &dyn Fn()| {
            counts.0.store(0, Ordering::SeqCst);
            call();
            counts.0.load(Ordering::SeqCst)
        };
        let made_dir = asked(&|| vfs::filename_mkdirat(&cred, &root, "dir").expect("mkdir"));
        let made_file = asked(&|| {
            assert!(
                vfs::do_file_open(&cred, &root, "file", &how).is_ok(),
                "the file must be made"
            );
        });
        vfs::vfs_fstatat(&cred, &root, "file", false).expect("the file must stand");
        let moved = asked(&|| {
            vfs::filename_renameat2(&cred, &root, "file", &root, "moved").expect("the move")
        });
        assert_eq!(
            (made_dir, made_file, moved),
            (1, 1, 1),
            "vfs::filename_mkdirat and vfs::do_file_open of a new name, and vfs::filename_renameat2 \
             of a held file onto one, must each look the new name up once"
        );
    }

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

        fn fill_super(&self, sb: &mut Filesystem) {
            sb.set_default_d_op(Arc::new(HookedDentry(self.stale.clone())));
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
    fn answers(
        prepare: impl Fn(Mount) -> Mount,
        access: Access,
    ) -> [Result<(), FilesystemError>; 3] {
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
                Err(FilesystemError::AlreadyExists),
                Err(FilesystemError::ReadOnlyFilesystem),
                Err(FilesystemError::ReadOnlyFilesystem),
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
                Err(FilesystemError::AlreadyExists),
                Err(FilesystemError::NotFound),
                Err(FilesystemError::NotFound),
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
            Err(FilesystemError::CrossesDevices),
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
                Err(FilesystemError::NotFound),
                Err(FilesystemError::InvalidInput),
                Err(FilesystemError::Unsupported),
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
            (Some(FilesystemError::FilesystemLoop), 0),
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
            Some(FilesystemError::PermissionDenied),
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
                Err(FilesystemError::InvalidInput),
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
                    Err(err) => Err(err),
                    _ => Ok(()),
                }
            })),
            ..HookedTmpfs::default()
        };
        let path = root_of(Filesystem::new(Arc::new(hooked)));
        root.set(path.clone()).ok();
        assert_eq!(
            vfs::filename_mkdirat(&Arc::default(), &path, "made"),
            Err(FilesystemError::Deadlock),
            "vfs::filename_mkdirat called by a Filesystem's own create_dir on the directory the outer \
             vfs::filename_mkdirat holds must answer Deadlock"
        );
    }

    // Nothing stands at `passing`, so a walk taking no lock would answer ENOENT;
    // only holding the directory while it finds the name meets the outer make.
    // @behavior RT-229
    #[test]
    fn a_walk_passing_through_a_directory_holds_it_to_find_a_name() {
        let root = Arc::new(OnceLock::<Path>::new());
        let hooked = HookedTmpfs {
            making: Some(Box::new({
                let root = root.clone();
                move || {
                    let start = root.get().expect("the root is set");
                    vfs::vfs_fstatat(&Arc::default(), start, "passing/through", false).map(|_| ())
                }
            })),
            ..HookedTmpfs::default()
        };
        let path = root_of(Filesystem::new(Arc::new(hooked)));
        root.set(path.clone()).ok();
        assert_eq!(
            vfs::filename_mkdirat(&Arc::default(), &path, "made"),
            Err(FilesystemError::Deadlock),
            "vfs::vfs_fstatat walking through the directory an outer vfs::filename_mkdirat holds \
             must answer Deadlock, as it holds that directory to find the name"
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
            (Err(FilesystemError::NotFound), Ok(())),
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
                Err(FilesystemError::NotFound),
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
        let error = Err;
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

        fn fill_super(&self, sb: &mut Filesystem) {
            sb.set_default_d_op(Arc::new(self.clone()));
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
            (Ok(()), Err(FilesystemError::NotFound), true, false),
            "vfs::filename_renameat2 of `a/x` that waited while another writer renamed it must \
             answer ENOENT and leave `a/moved`, which the Policy refuses, where it stands"
        );
    }
}
