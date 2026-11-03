//! Resolving a path beneath a directory, one name at a time, and the
//! calls that act on the name it ends at: opening it, making it, removing
//! it, and moving it, as Linux's `namei.c` holds both.
//!
//! Each step hands a Filesystem one name in a directory already held,
//! opened without following a link there, so nothing another writer swaps
//! in on the way leads the walk elsewhere. A link is followed only where
//! the walk itself reads where it points, within the directory the walk
//! started from, as `openat2`'s `RESOLVE_BENEATH` keeps a resolution: one
//! leading out of it, or to an absolute path, escapes it.

use std::collections::VecDeque;
use std::sync::Arc;

use super::attr::notify_change;
use super::cred::{Cred, FileSecurity, SecurityPath};
use super::dentry::{Dentry, RENAME_LOCK};
use super::file::{FMode, File, FileOperations};
use super::inode::{Finish, Inode, Mask, RenameFlag};
use super::mount::VfsMount;
use super::openat2::{OpenFlags, OpenHow};
use super::path::{Path, Start};
use super::super_block::SuperBlock;
use super::{FileType, FilesystemError, Iattr, Kstat};
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
    /// The directory the walk started from, which no `..` climbs above, as
    /// `nd->root` holds it for `RESOLVE_BENEATH`.
    root: Arc<Dentry>,
    /// Where `RENAME_LOCK` stood when the walk began, as `nd->r_seq`.
    r_seq: usize,
    /// The directory the walk stands in, which holds the last name.
    dir: Arc<Dentry>,
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
        &self.dir
    }

    /// `dentry` beneath the mount the walk stands on, as Linux's `struct path`
    /// pairs `nd->path.mnt` with a dentry.
    pub(crate) fn path_to(&self, dentry: Arc<Dentry>) -> Path {
        Path::root(&self.mnt).with_dentry(dentry)
    }

    /// Refuse what the walk ended on unless it still stands beneath where the
    /// walk started, as `complete_walk` asks `path_is_under` once a move may have
    /// carried a directory out from under it.
    fn complete_walk(&self) -> Result<(), FilesystemError> {
        let ended = self.path.as_ref().unwrap_or(&self.dir);
        match ended.is_subdir(&self.root) {
            true => Ok(()),
            false => Err(FilesystemError::Escape),
        }
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
    pub(crate) fn getattr(&self) -> Result<Kstat, FilesystemError> {
        self.path()?.getattr()
    }

    /// The path from the mount's root, read from the dentry the walk stands
    /// in as Linux's `d_path` reads one: each name spelled as the directory
    /// holding it stores it, the last name kept as given where nothing
    /// holds it.
    pub(crate) fn dentry_path(&self) -> String {
        let mut names = self.dentry().names();
        names.push(self.name.clone());
        names.retain(|name| !name.is_empty());
        names.join("/")
    }

    /// Answer `hook` for the path the walk ended on, as Linux's `security_path_*`
    /// is handed `struct path` once its call holds the name.
    fn on_path<T>(&self, hook: impl FnOnce(SecurityPath<'_>) -> T) -> T {
        hook(SecurityPath {
            mnt: &self.mnt,
            name: &|| self.dentry_path(),
        })
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
pub(crate) fn d_revalidate(
    sb: &SuperBlock,
    dir: &Dentry,
    dentry: &Arc<Dentry>,
) -> Result<bool, FilesystemError> {
    let valid = match dentry.ops().filter(|_| dentry.looked_up()) {
        Some(ops) => ops.revalidate(dir.inode_ops()?.as_ref(), dentry)?,
        None => true,
    };
    if !valid {
        sb.dentries().unhash(dentry);
    }
    Ok(valid)
}

/// `name` in `parent` as that directory stores it, and its dentry, for a call
/// holding that directory's lock to change the name, as Linux's
/// `lookup_one_qstr_excl` does.
pub(crate) fn lookup_one_qstr_excl(
    sb: &SuperBlock,
    parent: &Arc<Dentry>,
    name: &str,
) -> Result<(String, Arc<Dentry>), FilesystemError> {
    __lookup_slow(sb, parent, name)
}

/// `name` in `parent` as that directory stores it, and the dentry of that
/// spelling with what stands there recorded on it, as Linux's `__lookup_slow`
/// asks `->lookup` once among the walks holding the directory's lock shared, and
/// a case-insensitive filesystem answers through `d_add_ci`: every spelling
/// reaching one entry shares its dentry, and so its lock. A directory removed
/// through the VFS is asked about no name, as Linux refuses a dead one.
fn __lookup_slow(
    sb: &SuperBlock,
    parent: &Arc<Dentry>,
    name: &str,
) -> Result<(String, Arc<Dentry>), FilesystemError> {
    if parent.is_deaddir() {
        return Err(FilesystemError::NotFound);
    }
    let dentry_of = |name: &str| -> Result<Arc<Dentry>, FilesystemError> {
        match lookup_dcache(sb, parent, name)? {
            Some(held) => Ok(held),
            None => Ok(sb.dentries().alloc_parallel(parent, name)),
        }
    };
    let given = dentry_of(name)?;
    let (stored, dentry) = match given.lookup_inode(parent)? {
        None => (name.to_string(), given),
        Some(stored) => {
            let dentry = dentry_of(&stored)?;
            dentry.lookup_inode(parent)?;
            (stored, dentry)
        }
    };
    if !sb.dontcache() {
        sb.dentries().retain(&dentry);
    }
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
    Ok(__lookup_slow(sb, parent, name)?.1)
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
            FileType::Dir => {
                self.dir = dentry;
                Ok(())
            }
            _ => Err(FilesystemError::NotADirectory),
        }
    }

    /// Walk on through where the link `dentry` names points, before
    /// whatever is left of the path.
    fn pick_link(&mut self, dentry: &Dentry) -> Result<(), FilesystemError> {
        self.links += 1;
        if self.links > MAXSYMLINKS {
            return Err(FilesystemError::FilesystemLoop);
        }
        let target = dentry.get_link()?;
        if target.starts_with('/') {
            return Err(FilesystemError::Escape);
        }
        // A link pointing nowhere leaves the walk where it stands, as Linux's
        // `pick_link` makes it a pure jump.
        let target = match target.is_empty() {
            true => ".".to_string(),
            false => target,
        };
        for part in parts(&target).into_iter().rev() {
            self.pending.push_front(part);
        }
        Ok(())
    }

    /// Step to the directory above the one the walk stands in, wherever that
    /// one stands now, as `follow_dotdot` reads `d_parent`. Out of the one the
    /// walk started from, or the mount's root, no `..` holds a capability, as
    /// `RESOLVE_BENEATH` refuses a caller's and a link's alike.
    fn follow_dotdot(&mut self) -> Result<(), FilesystemError> {
        if Arc::ptr_eq(&self.dir, &self.root) {
            return Err(FilesystemError::Escape);
        }
        self.dir = self.dir.parent().ok_or(FilesystemError::Escape)?;
        // A move along the way may have carried `..` above where the walk started,
        // so the caller makes the call again, as `handle_dots` answers a scoped walk.
        if RENAME_LOCK.read_seqretry(self.r_seq) {
            return Err(FilesystemError::WouldBlock);
        }
        Ok(())
    }

    /// Walk every name but the last, which is left for the call to act on, as
    /// Linux's `link_path_walk` leaves `nd->last` and `nd->last_type`.
    fn link_path_walk(&mut self) -> Result<(), FilesystemError> {
        loop {
            inode_permission(&self.cred, self.mnt.sb(), self.dentry(), Mask::EXEC)?;
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

    /// Stand where a last `.` or `..` leads, as Linux's `handle_dots` does.
    fn handle_dots(&mut self) -> Result<(), FilesystemError> {
        if self.last_type == LastType::DotDot {
            self.follow_dotdot()?;
        }
        self.path = Some(self.dentry().clone());
        Ok(())
    }

    /// Find what the last name stands for, as Linux's `lookup_last` does: a
    /// trailing `/` follows a link there and asks for a directory, a link
    /// followed leaves another last name to find, and nothing standing there is
    /// ENOENT, as `step_into` answers before the walk is checked.
    fn lookup_last(&mut self, mut follow: bool) -> Result<(), FilesystemError> {
        loop {
            if self.last_type != LastType::Norm {
                return self.handle_dots();
            }
            if self.trailing_slash {
                follow = true;
                self.lookup_directory = true;
            }
            let parent = self.dentry().clone();
            let held = parent.inode_lock_shared()?;
            let child = self.lookup_last_locked()?;
            drop(held);
            if child.inode().is_none() {
                return Err(FilesystemError::NotFound);
            }
            if !follow || child.kind() != Ok(FileType::Symlink) {
                return Ok(());
            }
            self.pick_link(&child)?;
            self.link_path_walk()?;
        }
    }

    /// Find the last name of an open, as Linux's `open_last_lookups` does. One
    /// that makes no file finds it as any call does. One that may make it finds
    /// it under the directory's lock it makes the file under, and is handed that
    /// lock, as `lookup_open` holds it; a trailing `/` refuses what it would make,
    /// as `lookup_fast_for_open` does.
    fn open_last_lookups(
        &mut self,
        follow: bool,
        create: bool,
    ) -> Result<Option<RwSemaphoreGuard>, FilesystemError> {
        if !create {
            self.lookup_last(follow)?;
            return Ok(None);
        }
        loop {
            if self.last_type != LastType::Norm {
                self.handle_dots()?;
                return Ok(None);
            }
            if self.trailing_slash {
                return Err(FilesystemError::IsADirectory);
            }
            let held = self.dentry().inode_lock()?;
            let child = self.lookup_last_locked()?;
            if !follow || child.kind() != Ok(FileType::Symlink) {
                return Ok(Some(held));
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
/// Filesystem can hold. It answers before a walk looks up where it starts, as
/// Linux's syscalls take the name before `path_init` takes the descriptor.
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

/// Whether `cred` may do what `mask` asks of what `dentry` names, as Linux's
/// `inode_permission` asks `->permission` and then the security modules.
fn inode_permission(
    cred: &Cred,
    sb: &SuperBlock,
    dentry: &Dentry,
    mask: Mask,
) -> Result<(), FilesystemError> {
    sb_permission(sb, dentry, mask)?;
    dentry.inode_ops()?.permission(dentry, mask)?;
    cred.security_inode_permission(dentry, mask)
}

/// Refuse a write to a file, a directory, or a link of a read-only `sb`, as
/// Linux's `sb_permission` does before any Filesystem or module is asked.
fn sb_permission(sb: &SuperBlock, dentry: &Dentry, mask: Mask) -> Result<(), FilesystemError> {
    if !mask.contains(Mask::WRITE) || !sb.rdonly() {
        return Ok(());
    }
    match dentry.kind()? {
        FileType::RegFile | FileType::Dir | FileType::Symlink => {
            Err(FilesystemError::ReadOnlyFilesystem)
        }
        _ => Ok(()),
    }
}

/// The walk of `path` beneath `start`, ready to begin, as Linux's
/// `set_nameidata` and `path_init` make one. A path no call can take answers
/// first, wherever it starts; an absolute one is not the caller's to reach, so
/// no start is looked for; and a walk starts from nothing but a directory.
fn set_nameidata(
    cred: &Arc<Cred>,
    start: &impl Start,
    path: &str,
) -> Result<Nameidata, FilesystemError> {
    getname(path)?;
    if path.starts_with('/') {
        return Err(FilesystemError::Escape);
    }
    let start = start.path_init()?;
    if start.dentry().kind()? != FileType::Dir {
        return Err(FilesystemError::NotADirectory);
    }
    Ok(Nameidata {
        mnt: start.mnt().clone(),
        cred: cred.clone(),
        root: start.dentry().clone(),
        r_seq: RENAME_LOCK.read_seqbegin(),
        dir: start.dentry().clone(),
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
    start: &impl Start,
    path: &str,
) -> Result<Nameidata, FilesystemError> {
    let mut nd = set_nameidata(cred, start, path)?;
    nd.link_path_walk()?;
    nd.complete_walk()?;
    Ok(nd)
}

/// `path` beneath `start`, walked to what it names, as Linux's `filename_lookup`.
/// Where `follow` asks, or a trailing `/` does, a link at the end is followed
/// too. Nothing standing at the last name answers ENOENT, as a walk does, and
/// anything but a directory where `LOOKUP_DIRECTORY` asks for one ENOTDIR.
pub(crate) fn filename_lookup(
    cred: &Arc<Cred>,
    start: &impl Start,
    path: &str,
    follow: bool,
) -> Result<Nameidata, FilesystemError> {
    let mut nd = set_nameidata(cred, start, path)?;
    nd.link_path_walk()?;
    nd.lookup_last(follow)?;
    nd.complete_walk()?;
    let kind = nd.kind()?;
    if nd.lookup_directory && kind != FileType::Dir {
        return Err(FilesystemError::NotADirectory);
    }
    Ok(nd)
}

/// Open `path` beneath `start` as `op` asks, as Linux's `do_file_open`
/// walks to the last name and `do_open` opens it. It hands no descriptor out,
/// as whoever holds a table of them reserves one first, as Linux's `FD_ADD`
/// does, so a path no call can take answers only once a descriptor is held.
pub fn do_file_open(
    cred: &Arc<Cred>,
    start: &impl Start,
    path: &str,
    op: &OpenFlags,
) -> Result<File, FilesystemError> {
    let how = op.open_flag();
    let mut nd = set_nameidata(cred, start, path)?;
    nd.lookup_directory = op.lookup_directory();
    nd.link_path_walk()?;
    let held = nd.open_last_lookups(op.lookup_follow(), how.create)?;
    let child = nd.path()?;
    // Only a create reaches the last name with nothing there, as
    // `open_last_lookups` refuses it otherwise.
    let opened = match child.inode() {
        None => lookup_open(&nd, &child, how)?,
        Some(_) => Opened::default(),
    };
    drop(held);
    let (f_op, security) = do_open(&nd, &child, opened, op)?;
    Ok(File::new(nd.path_to(child), f_op, how, cred).with_security(security))
}

/// What `lookup_open` leaves `do_open`, as Linux's open `file` carries
/// `FMODE_OPENED` and `FMODE_CREATED` out of it.
#[derive(Default)]
struct Opened {
    file: Option<(Box<dyn FileOperations>, Vec<FileSecurity>)>,
    created: bool,
}

/// What an open of `how` is for, marked as made by it where it was, as
/// `->atomic_open` and `lookup_open` set `FMODE_CREATED`.
fn f_mode(how: &OpenHow, created: bool) -> FMode {
    match created {
        true => FMode::of(how).with(FMode::CREATED),
        false => FMode::of(how),
    }
}

/// Make the file an open names where nothing stands, as Linux's `lookup_open`
/// asks the mount and `may_o_create` before `->atomic_open` or `->create`, under
/// the directory's lock its caller holds. A Filesystem opening the file too is
/// asked of the security modules here, as its `finish_open` reaches
/// `do_dentry_open`.
fn lookup_open(nd: &Nameidata, child: &Dentry, how: &OpenHow) -> Result<Opened, FilesystemError> {
    nd.mnt.mnt_want_write()?;
    nd.on_path(|path| nd.cred.security_path_mknod(path))?;
    may_create_dentry(&nd.cred, nd.mnt.sb(), nd.dentry(), child)?;
    match atomic_open(nd.dentry(), child, how)? {
        Finish::Open { file, created } => {
            let kind = child.kind()?;
            let mode = f_mode(how, created);
            let security = nd.on_path(|path| nd.cred.security_file_open(path, kind, mode, how))?;
            Ok(Opened {
                file: Some((file, security)),
                created,
            })
        }
        Finish::NoOpen { created } => Ok(Opened {
            file: None,
            created,
        }),
    }
}

/// Open what the walk ended on once the directory's lock is gone, as Linux's
/// `do_open`. An entry that stood is walked to completion and answers first,
/// while a file the open made asks for nothing but the open and is never
/// truncated. A truncate asks the mount before `may_open`, a write asks it
/// after for anything but a special file, as `do_dentry_open` takes write
/// access, and only a regular file is emptied, as `handle_truncate` asks the
/// security modules first. What a Filesystem opened already is not opened
/// again, as `FMODE_OPENED` skips `vfs_open`.
fn do_open(
    nd: &Nameidata,
    child: &Dentry,
    opened: Opened,
    op: &OpenFlags,
) -> Result<(Box<dyn FileOperations>, Vec<FileSecurity>), FilesystemError> {
    let how = op.open_flag();
    let created = opened.created;
    if opened.file.is_none() && !created {
        nd.complete_walk()?;
    }
    // The walk followed every link the call asked it to, so a link at the
    // end now is one swapped in since, and is opened as itself.
    let kind = child.kind()?;
    let refusal = match kind {
        _ if how.create && how.exclusive && !created => Some(FilesystemError::AlreadyExists),
        FileType::Dir if how.create => Some(FilesystemError::IsADirectory),
        FileType::Dir => None,
        _ if nd.lookup_directory => Some(FilesystemError::NotADirectory),
        _ => None,
    };
    if let Some(error) = refusal {
        return Err(error);
    }
    let (acc_mode, truncate) = match created {
        true => (Mask::NONE, false),
        false => (op.acc_mode(), how.truncate && kind == FileType::RegFile),
    };
    if truncate {
        nd.mnt.mnt_want_write()?;
    }
    may_open(&nd.cred, nd.mnt.sb(), child, kind, acc_mode)?;
    let (f_op, security) = match opened.file {
        Some(file) => file,
        None => {
            if how.write && !kind.is_special() {
                nd.mnt.mnt_want_write()?;
            }
            let mode = f_mode(how, created);
            let security = nd.on_path(|path| nd.cred.security_file_open(path, kind, mode, how))?;
            (child.open(how)?, security)
        }
    };
    if truncate {
        nd.cred.security_file_truncate(&security)?;
        let held = child.inode_lock()?;
        notify_change(child, &held, &Iattr::open_truncate())?;
    }
    Ok((f_op, security))
}

/// Refuse to open what stands at the path as Linux's `may_open` does: a
/// link, which the walk would have followed if asked, a directory for
/// writing, and anything `inode_permission` refuses for what the open asks.
fn may_open(
    cred: &Cred,
    sb: &SuperBlock,
    child: &Dentry,
    kind: FileType,
    mask: Mask,
) -> Result<(), FilesystemError> {
    match kind {
        FileType::Symlink => return Err(FilesystemError::FilesystemLoop),
        FileType::Dir if mask.contains(Mask::WRITE) => return Err(FilesystemError::IsADirectory),
        _ => {}
    }
    inode_permission(cred, sb, child, Mask::OPEN | mask)
}

/// Give what `old_path` beneath `old_start` names another name at `new_path`
/// beneath `new_start`, as Linux's `filename_linkat` walks to the first, then
/// to where the second is made, and refuses a move across mounts once it knows
/// nothing stands there. A link at the end of `old_path` is followed where
/// `follow` asks.
pub fn filename_linkat(
    cred: &Arc<Cred>,
    old_start: &impl Start,
    old_path: &str,
    new_start: &impl Start,
    new_path: &str,
    follow: bool,
) -> Result<(), FilesystemError> {
    let old = filename_lookup(cred, old_start, old_path, follow)?;
    let target = old.path()?;
    let mut new = filename_parentat(cred, new_start, new_path)?;
    let (_held, child) = filename_create(&mut new, false)?;
    if !Arc::ptr_eq(&old.mnt, &new.mnt) {
        return Err(FilesystemError::CrossesDevices);
    }
    old.on_path(|old| new.on_path(|new| cred.security_path_link(old, new)))?;
    vfs_link(cred, new.mnt.sb(), &target, new.dentry(), &child)
}

/// Make the link `path` beneath `start` names, pointing at `target`, as
/// Linux's `filename_symlinkat` does.
pub fn filename_symlinkat(
    cred: &Arc<Cred>,
    target: &str,
    start: &impl Start,
    path: &str,
) -> Result<(), FilesystemError> {
    getname(target)?;
    let mut nd = filename_parentat(cred, start, path)?;
    let (_held, child) = filename_create(&mut nd, false)?;
    nd.on_path(|path| cred.security_path_symlink(path))?;
    vfs_symlink(cred, nd.mnt.sb(), nd.dentry(), &child, target)
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
    if child.inode().is_some() {
        return Err(FilesystemError::AlreadyExists);
    }
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
pub fn filename_mkdirat(
    cred: &Arc<Cred>,
    start: &impl Start,
    path: &str,
) -> Result<(), FilesystemError> {
    let mut nd = filename_parentat(cred, start, path)?;
    let (_held, child) = filename_create(&mut nd, true)?;
    nd.on_path(|path| cred.security_path_mkdir(path))?;
    vfs_mkdir(cred, nd.mnt.sb(), nd.dentry(), &child)
}

/// Remove the empty directory `path` beneath `start`, as Linux's
/// `filename_rmdir` does.
pub fn filename_rmdir(
    cred: &Arc<Cred>,
    start: &impl Start,
    path: &str,
) -> Result<(), FilesystemError> {
    let mut nd = filename_parentat(cred, start, path)?;
    match nd.last_type {
        LastType::Norm => {}
        LastType::Dot => return Err(FilesystemError::InvalidInput),
        LastType::DotDot => return Err(FilesystemError::DirectoryNotEmpty),
    }
    nd.mnt.mnt_want_write()?;
    let (_held, child) = start_dirop(&mut nd)?;
    nd.on_path(|path| cred.security_path_rmdir(path))?;
    vfs_rmdir(cred, nd.mnt.sb(), nd.dentry(), &child)
}

/// Remove the file `path` beneath `start`, as Linux's `filename_unlinkat` does.
/// It follows no link at the end, so a trailing `/` is answered by the name
/// itself once it is found: no file removes as a directory.
pub fn filename_unlinkat(
    cred: &Arc<Cred>,
    start: &impl Start,
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
            Ok(FileType::Dir) => FilesystemError::IsADirectory,
            _ => FilesystemError::NotADirectory,
        });
    }
    nd.on_path(|path| cred.security_path_unlink(path))?;
    vfs_unlink(cred, nd.mnt.sb(), nd.dentry(), &child)
}

/// Move `from_path` beneath `from` to `to_path` beneath `to`. A rename
/// stays within one mount; across two it crosses devices, as across two
/// mounted devices. A security hook may refuse the move of a directory, as
/// Linux's `security_path_rename` asks before the move.
pub fn filename_renameat2(
    cred: &Arc<Cred>,
    from: &impl Start,
    from_path: &str,
    to: &impl Start,
    to_path: &str,
) -> Result<(), FilesystemError> {
    let mut source = filename_parentat(cred, from, from_path)?;
    let mut target = filename_parentat(cred, to, to_path)?;
    if !Arc::ptr_eq(&source.mnt, &target.mnt) {
        return Err(FilesystemError::CrossesDevices);
    }
    if source.last_type != LastType::Norm || target.last_type != LastType::Norm {
        return Err(FilesystemError::ResourceBusy);
    }
    source.mnt.mnt_want_write()?;
    let mnt = source.mnt.clone();
    let sb = mnt.sb();
    let rd = __start_renaming(
        sb,
        (source.dentry(), &source.name),
        (target.dentry(), &target.name),
        None,
    )?;
    source.name = rd.old_dentry.name();
    target.name = rd.new_dentry.name();
    // Unless the source is a directory, a trailing `/` on either side is
    // refused, as `filename_renameat2` refuses it.
    let is_dir = rd.old_dentry.kind()? == FileType::Dir;
    if !is_dir && (source.trailing_slash || target.trailing_slash) {
        return Err(FilesystemError::NotADirectory);
    }
    source.on_path(|old| {
        target.on_path(|new| cred.security_path_rename(old, new, (&rd.old_dentry, &rd.new_dentry)))
    })?;
    vfs_rename(cred, sb, &rd)
}

/// Hold both directories a move between them changes exclusive, as Linux's
/// `lock_rename` does: the Filesystem's rename lock first where they
/// differ, so no other such move changes which stands beneath the other,
/// then the one above the other before it, as a removal holds the
/// directory above the one it removes first, as `lock_two_directories`.
fn lock_rename(
    sb: &SuperBlock,
    old_dir: &Arc<Dentry>,
    new_dir: &Arc<Dentry>,
) -> Result<Vec<RwSemaphoreGuard>, FilesystemError> {
    if Arc::ptr_eq(old_dir, new_dir) {
        return Ok(vec![old_dir.inode_lock()?]);
    }
    let rename = sb.rename_mutex().down_write()?;
    let (first, second) = match old_dir.is_subdir(new_dir) {
        true => (new_dir, old_dir),
        false => (old_dir, new_dir),
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
/// Linux's `__start_renaming` does through `lock_rename`, so no other move
/// changes what either name stands for before the call acts on it. Nothing at
/// the source, and at the target what `flag` will not take, is refused as the
/// flags `__start_renaming` looks each name up with refuse it. So is a move
/// whose source stands above its target directory, and one whose target stands
/// above its source directory, which only an exchange may not empty.
pub(crate) fn __start_renaming(
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
pub(crate) fn vfs_rename(
    cred: &Cred,
    sb: &SuperBlock,
    rd: &RenameData,
) -> Result<(), FilesystemError> {
    let (old_dir, old) = (&rd.old_parent, &rd.old_dentry);
    let (new_dir, new) = (&rd.new_parent, &rd.new_dentry);
    let flag = rd.flag;
    let moving = old.kind().ok();
    let target = new.kind().ok();
    if Arc::ptr_eq(old, new) || old.names_the_entry_of(new) {
        return Ok(());
    }
    let is_dir = moving == Some(FileType::Dir);
    may_delete_dentry(cred, sb, old_dir, moving, is_dir)?;
    match (target, flag) {
        (None, _) => may_create_dentry(cred, sb, new_dir, new)?,
        (target, Some(RenameFlag::Exchange)) => {
            may_delete_dentry(cred, sb, new_dir, target, target == Some(FileType::Dir))?
        }
        (target, _) => may_delete_dentry(cred, sb, new_dir, target, is_dir)?,
    }
    // A directory changing its parent changes its `..`, so it is asked to change.
    if !Arc::ptr_eq(old_dir, new_dir) {
        if is_dir {
            inode_permission(cred, sb, old, Mask::WRITE)?;
        }
        if flag == Some(RenameFlag::Exchange) && target == Some(FileType::Dir) {
            inode_permission(cred, sb, new, Mask::WRITE)?;
        }
    }
    let _held = lock_moved(
        (old, is_dir),
        (new, target == Some(FileType::Dir)),
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
    if target == Some(FileType::Dir) {
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
    cred: &Cred,
    sb: &SuperBlock,
    dir: &Dentry,
    victim: &Arc<Dentry>,
) -> Result<(), FilesystemError> {
    may_delete_dentry(cred, sb, dir, victim.kind().ok(), true)?;
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
    cred: &Cred,
    sb: &SuperBlock,
    dir: &Dentry,
    victim: &Arc<Dentry>,
) -> Result<(), FilesystemError> {
    may_delete_dentry(cred, sb, dir, victim.kind().ok(), false)?;
    let _victim = victim.inode_lock()?;
    dir.inode_ops()?.unlink(victim)?;
    sb.dentries().unhash(victim);
    Ok(())
}

/// Make the directory `child` in `dir`, as Linux's `vfs_mkdir` does once its
/// caller holds `dir`. A Filesystem that records nothing on `child` answers for
/// it at the next walk, as its `revalidate` or its uncached names do.
pub(crate) fn vfs_mkdir(
    cred: &Cred,
    sb: &SuperBlock,
    dir: &Dentry,
    child: &Dentry,
) -> Result<(), FilesystemError> {
    may_create_dentry(cred, sb, dir, child)?;
    dir.inode_ops()?.mkdir(child)
}

/// Make the file `child` in `dir`, opened as `how` asks where its Filesystem
/// opens it too, as Linux's `atomic_open` hands `open_flag` on once its caller
/// holds `dir` and asked `may_o_create`.
pub(crate) fn atomic_open(
    dir: &Dentry,
    child: &Dentry,
    how: &OpenHow,
) -> Result<Finish, FilesystemError> {
    dir.inode_ops()?.atomic_open(child, how)
}

/// Make the file `child` in `dir`, as Linux's `vfs_create` does once its caller
/// holds `dir`.
pub(crate) fn vfs_create(
    cred: &Cred,
    sb: &SuperBlock,
    dir: &Dentry,
    child: &Dentry,
) -> Result<(), FilesystemError> {
    may_create_dentry(cred, sb, dir, child)?;
    dir.inode_ops()?.create(child)
}

/// Give what `old` names another name, `child` in `dir`, as Linux's `vfs_link`
/// does once its caller holds `dir`: a directory is refused, and what `old`
/// names is held, so a removal landing first leaves no name to give another.
fn vfs_link(
    cred: &Cred,
    sb: &SuperBlock,
    old: &Dentry,
    dir: &Dentry,
    child: &Dentry,
) -> Result<(), FilesystemError> {
    may_create_dentry(cred, sb, dir, child)?;
    if old.kind()? == FileType::Dir {
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
fn vfs_symlink(
    cred: &Cred,
    sb: &SuperBlock,
    dir: &Dentry,
    child: &Dentry,
    target: &str,
) -> Result<(), FilesystemError> {
    may_create_dentry(cred, sb, dir, child)?;
    dir.inode_ops()?.symlink(child, target)
}

/// Refuse to make an entry at `child` in `dir` where its lookup found one, or
/// once `dir` was removed, as Linux's `may_create_dentry` reads `d_inode`.
pub(crate) fn may_create_dentry(
    cred: &Cred,
    sb: &SuperBlock,
    dir: &Dentry,
    child: &Dentry,
) -> Result<(), FilesystemError> {
    if child.inode().is_some() {
        return Err(FilesystemError::AlreadyExists);
    }
    if dir.is_deaddir() {
        return Err(FilesystemError::NotFound);
    }
    inode_permission(cred, sb, dir, Mask::WRITE | Mask::EXEC)
}

/// Refuse to remove `victim`, what stands at a name in `dir`, where
/// nothing does, where `inode_permission` refuses a change to `dir`, where it is
/// not the kind `is_dir` asks for, or once `dir` was removed, as Linux's
/// `may_delete_dentry` refuses.
fn may_delete_dentry(
    cred: &Cred,
    sb: &SuperBlock,
    dir: &Dentry,
    victim: Option<FileType>,
    is_dir: bool,
) -> Result<(), FilesystemError> {
    let Some(victim) = victim else {
        return Err(FilesystemError::NotFound);
    };
    inode_permission(cred, sb, dir, Mask::WRITE | Mask::EXEC)?;
    match (is_dir, victim == FileType::Dir) {
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
        DentryOperations, FileOperations, FileSystemType, FileType, Filesystem, FilesystemError,
        Finish, InodeOperations, Kstat, Lookup, Tmpfs,
    };
    use crate::mount::{Access, Mount, Policy};
    use crate::sched::testing::count_waits;
    use crate::vfs::kern_path::{kern_path, KernPath};
    use crate::vfs::{
        self, Cred, Dentry, FMode, FileSecurity, Inode, Mask, OpenHow, Path, RenameFlag,
        SecurityOperations, SecurityPath,
    };

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

    impl FileSystemType for LinksFile {
        fn fill_super(self: Arc<Self>, sb: &mut Filesystem) -> Result<(), FilesystemError> {
            vfs::simple_fill_super(sb, self)
        }
    }

    impl InodeOperations for LinksFile {
        fn getattr(&self, dentry: &Dentry) -> Result<Kstat, FilesystemError> {
            Ok(match dentry.parent() {
                Some(_) => Kstat::new(FileType::RegFile, 0).with_nlink(self.names),
                None => Kstat::new(FileType::Dir, 0),
            })
        }

        fn lookup(&self, dentry: &Dentry) -> Result<Lookup, FilesystemError> {
            if dentry.name() != "file" {
                return Ok(Lookup::Absent);
            }
            let file = Kstat::new(FileType::RegFile, 0);
            Ok(Lookup::Found(Inode::new(&file, Arc::new(self.clone()))))
        }

        fn link(&self, _old: &Dentry, _dentry: &Dentry) -> Result<(), FilesystemError> {
            self.linked.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    /// The root of an `AskedOfAbsent`, with what counts its lookups and
    /// revalidations.
    fn counted_lookups(deleted: bool, stands: bool) -> (Path, Arc<AtomicUsize>, Arc<AtomicUsize>) {
        let lookups = Arc::new(AtomicUsize::new(0));
        let revalidations = Arc::new(AtomicUsize::new(0));
        let ops = AskedOfAbsent {
            deleted,
            stands,
            lookups: lookups.clone(),
            revalidations: revalidations.clone(),
        };
        let mount = Mount::new(
            "/data",
            Filesystem::new(Arc::new(ops)).expect("the instance must stand"),
            Access::ReadWrite,
        )
        .expect("/data is a guest path");
        (Path::root(mount.mnt()), lookups, revalidations)
    }

    /// A Filesystem whose one answer for every name is `stands`, counting what it
    /// is asked, keeping a name unless `deleted`.
    #[derive(Clone)]
    struct AskedOfAbsent {
        deleted: bool,
        stands: bool,
        lookups: Arc<AtomicUsize>,
        revalidations: Arc<AtomicUsize>,
    }

    impl DentryOperations for AskedOfAbsent {
        fn revalidate(
            &self,
            _dir: &dyn InodeOperations,
            _dentry: &Dentry,
        ) -> Result<bool, FilesystemError> {
            self.revalidations.fetch_add(1, Ordering::SeqCst);
            Ok(true)
        }

        fn delete(&self, _dentry: &Dentry) -> bool {
            self.deleted
        }
    }

    impl FileSystemType for AskedOfAbsent {
        fn fill_super(self: Arc<Self>, sb: &mut Filesystem) -> Result<(), FilesystemError> {
            sb.set_default_d_op(self.clone());
            vfs::simple_fill_super(sb, self)
        }
    }

    impl InodeOperations for AskedOfAbsent {
        fn getattr(&self, _dentry: &Dentry) -> Result<Kstat, FilesystemError> {
            Ok(Kstat::new(FileType::Dir, 0))
        }

        fn lookup(&self, _dentry: &Dentry) -> Result<Lookup, FilesystemError> {
            self.lookups.fetch_add(1, Ordering::SeqCst);
            Ok(match self.stands {
                true => {
                    let file = Kstat::new(FileType::RegFile, 0);
                    Lookup::Found(Inode::new(&file, Arc::new(self.clone())))
                }
                false => Lookup::Absent,
            })
        }
    }

    // @behavior RT-245
    #[test]
    fn a_name_nothing_stands_at_stays_for_the_next_walk() {
        let cred = Arc::default();
        let stat_twice = |root: &Path| {
            let first = vfs::vfs_fstatat(&cred, root, "gone", false);
            let second = vfs::vfs_fstatat(&cred, root, "gone", false);
            (first.err(), second.err())
        };
        let (root, lookups, revalidations) = counted_lookups(false, false);
        assert_eq!(
            (
                stat_twice(&root),
                lookups.load(Ordering::SeqCst),
                revalidations.load(Ordering::SeqCst)
            ),
            (
                (
                    Some(FilesystemError::NotFound),
                    Some(FilesystemError::NotFound)
                ),
                1,
                1
            ),
            "vfs::vfs_fstatat twice of a name nothing stands at must ask ->lookup once and \
             revalidate what it kept, as retain_dentry keeps a negative dentry"
        );
        let (root, lookups, _) = counted_lookups(true, false);
        stat_twice(&root);
        assert_eq!(
            lookups.load(Ordering::SeqCst),
            2,
            "a Filesystem whose delete lets a name go must be asked again for it"
        );
    }

    // Holding it shared again past the waiting writer stands in for what a walk
    // does when a Ruby Filesystem reaches its own Mount, which must never hang.
    // @behavior RT-253
    #[test]
    fn a_writer_goes_ahead_of_readers_arriving_after_it() {
        use crate::locking::RwSemaphore;
        let rwsem: Arc<RwSemaphore> = Arc::default();
        let order = Arc::new(Mutex::new(Vec::new()));
        let held = rwsem.down_read().expect("the first reader holds it");
        let writer = {
            let (rwsem, order) = (rwsem.clone(), order.clone());
            thread::spawn(move || {
                let _held = rwsem.down_write().expect("the writer holds it");
                order.lock().expect("never poisoned").push("writer");
            })
        };
        crate::sched::testing::wait_until("the writer queueing behind the reader", || {
            rwsem.writer_waits()
        });
        let reader = {
            let (rwsem, order) = (rwsem.clone(), order.clone());
            thread::spawn(move || {
                let _held = rwsem.down_read().expect("the reader holds it");
                order.lock().expect("never poisoned").push("reader");
            })
        };
        let again = rwsem.down_read().map(drop);
        thread::sleep(Duration::from_millis(50));
        let before = order.lock().expect("never poisoned").clone();
        drop(held);
        crate::sched::testing::join("the writer", writer);
        crate::sched::testing::join("the reader", reader);
        assert_eq!(
            (again, before, order.lock().expect("never poisoned").clone()),
            (Ok(()), Vec::new(), vec!["writer", "reader"]),
            "a reader arriving behind a waiting writer must wait for it, as \
             rwsem_down_read_slowpath queues, while one already holding it takes it again"
        );
    }

    /// A security module recording the hooks a call reaches, by name.
    #[derive(Default)]
    struct RecordsHooks(Mutex<Vec<String>>);

    impl RecordsHooks {
        fn record(&self, hook: &str) {
            self.0
                .lock()
                .expect("never poisoned")
                .push(hook.to_string());
        }
    }

    impl vfs::SecurityOperations for RecordsHooks {
        fn path_mknod(&self, _path: vfs::SecurityPath<'_>) -> Result<(), FilesystemError> {
            self.record("path_mknod");
            Ok(())
        }

        fn path_mkdir(&self, _path: vfs::SecurityPath<'_>) -> Result<(), FilesystemError> {
            self.record("path_mkdir");
            Ok(())
        }

        fn path_rmdir(&self, _path: vfs::SecurityPath<'_>) -> Result<(), FilesystemError> {
            self.record("path_rmdir");
            Ok(())
        }

        fn path_unlink(&self, _path: vfs::SecurityPath<'_>) -> Result<(), FilesystemError> {
            self.record("path_unlink");
            Ok(())
        }

        fn path_rename(
            &self,
            _old: vfs::SecurityPath<'_>,
            _new: vfs::SecurityPath<'_>,
            _dentries: (&Arc<Dentry>, &Arc<Dentry>),
        ) -> Result<(), FilesystemError> {
            self.record("path_rename");
            Ok(())
        }

        fn file_open(
            &self,
            _path: vfs::SecurityPath<'_>,
            _kind: FileType,
            _mode: vfs::FMode,
            _how: &OpenHow,
        ) -> Result<vfs::FileSecurity, FilesystemError> {
            self.record("file_open");
            Ok(None)
        }

        fn file_truncate(&self, _kept: &vfs::FileSecurity) -> Result<(), FilesystemError> {
            self.record("file_truncate");
            Ok(())
        }
    }

    // @behavior RT-249
    #[test]
    fn each_call_reaches_the_hook_linux_names_for_it() {
        let hooks = Arc::new(RecordsHooks::default());
        let cred = vfs::Cred::new(vec![hooks.clone()]);
        let mount = Mount::new(
            "/data",
            Filesystem::new(Arc::new(Tmpfs::new())).expect("the instance must stand"),
            Access::ReadWrite,
        )
        .expect("/data is a guest path");
        let root = Path::root(mount.mnt());
        let write = OpenHow {
            write: true,
            create: true,
            ..OpenHow::default()
        };
        let reopen = OpenHow {
            write: true,
            truncate: true,
            ..OpenHow::default()
        };
        vfs::filename_mkdirat(&cred, &root, "d").expect("mkdir");
        vfs::filename_rmdir(&cred, &root, "d").expect("rmdir");
        vfs::kern_path::filp_open(&cred, &root, "f", &write).expect("the file is made");
        let mut file =
            vfs::kern_path::filp_open(&cred, &root, "f", &reopen).expect("the file opens");
        file.truncate(0).expect("truncate");
        vfs::filename_renameat2(&cred, &root, "f", &root, "g").expect("rename");
        vfs::filename_unlinkat(&cred, &root, "g").expect("unlink");
        assert_eq!(
            *hooks.0.lock().expect("never poisoned"),
            [
                "path_mkdir",
                "path_rmdir",
                "path_mknod",
                "file_open",
                "file_open",
                "file_truncate",
                "file_truncate",
                "path_rename",
                "path_unlink",
            ],
            "each call through the VFS must reach the security hook Linux names for it, the \
             open of a file the call just made included, as do_dentry_open asks security_file_open"
        );
    }

    // @behavior RT-250
    #[test]
    fn a_module_refusing_what_a_walk_asks_refuses_the_walk() {
        struct RefusesExec;

        impl vfs::SecurityOperations for RefusesExec {
            fn inode_permission(&self, dentry: &Dentry, mask: Mask) -> Result<(), FilesystemError> {
                match dentry.name() == "d" && mask.contains(Mask::EXEC) {
                    true => Err(FilesystemError::PermissionDenied),
                    false => Ok(()),
                }
            }
        }

        let tmpfs: Tmpfs = [("d/f", "x")].into_iter().collect();
        let mount = Mount::new(
            "/data",
            Filesystem::new(Arc::new(tmpfs)).expect("the instance must stand"),
            Access::Read,
        )
        .expect("/data is a guest path");
        let root = Path::root(mount.mnt());
        let cred = vfs::Cred::new(vec![Arc::new(RefusesExec)]);
        assert_eq!(
            vfs::vfs_fstatat(&cred, &root, "d/f", false).err(),
            Some(FilesystemError::PermissionDenied),
            "vfs_fstatat through a directory a module refuses to pass must answer EACCES, as \
             security_inode_permission answers may_lookup"
        );
    }

    /// A Filesystem whose `fill_super` names its root.
    struct NamesItsRoot;

    impl FileSystemType for NamesItsRoot {
        fn fill_super(self: Arc<Self>, sb: &mut Filesystem) -> Result<(), FilesystemError> {
            let root = Kstat::new(FileType::Dir, 0).with_identity(7, 11);
            sb.d_make_root(Inode::new(&root, self));
            Ok(())
        }
    }

    /// A Filesystem that cannot stand, or stands with no root.
    struct NeverStands(Option<FilesystemError>);

    impl FileSystemType for NeverStands {
        fn fill_super(self: Arc<Self>, _sb: &mut Filesystem) -> Result<(), FilesystemError> {
            match &self.0 {
                Some(error) => Err(error.clone()),
                None => Ok(()),
            }
        }
    }

    // @behavior RT-254
    #[test]
    fn an_instance_that_cannot_stand_answers_why() {
        let answer = |fs: NeverStands| Filesystem::new(Arc::new(fs)).err();
        assert_eq!(
            (
                answer(NeverStands(Some(FilesystemError::PermissionDenied))),
                answer(NeverStands(None)),
            ),
            (
                Some(FilesystemError::PermissionDenied),
                Some(FilesystemError::InvalidInput),
            ),
            "Filesystem::new must answer what fill_super answers, and EINVAL for a fill_super \
             that made no root, and make no instance, as vfs_get_tree refuses one"
        );
    }

    impl InodeOperations for NamesItsRoot {
        fn getattr(&self, _dentry: &Dentry) -> Result<Kstat, FilesystemError> {
            Ok(Kstat::new(FileType::Dir, 0))
        }
    }

    // @behavior RT-247
    #[test]
    fn a_root_stands_for_what_fill_super_made() {
        let identity = |fs: Arc<dyn FileSystemType>| {
            let mount = Mount::new(
                "/data",
                Filesystem::new(fs).expect("the instance must stand"),
                Access::Read,
            )
            .expect("/data is a guest path");
            let inode = Path::root(mount.mnt()).dentry().inode();
            inode.and_then(|inode| inode.identity())
        };
        let reported = Kstat::new(FileType::Dir, 0).with_identity(7, 11);
        assert_eq!(
            (
                identity(Arc::new(NamesItsRoot)),
                identity(Arc::new(CreatesOnly::default()))
            ),
            (vfs::Identity::of(&reported), None),
            "Filesystem::new must stand on the root fill_super made, with no identity for one \
             made through simple_fill_super, as fill_super makes s_root"
        );
    }

    /// A Filesystem handing one inode for every name.
    #[derive(Clone)]
    struct SharesInode(Arc<OnceLock<Arc<Inode>>>);

    impl FileSystemType for SharesInode {
        fn fill_super(self: Arc<Self>, sb: &mut Filesystem) -> Result<(), FilesystemError> {
            vfs::simple_fill_super(sb, self)
        }
    }

    impl InodeOperations for SharesInode {
        fn getattr(&self, _dentry: &Dentry) -> Result<Kstat, FilesystemError> {
            Ok(Kstat::new(FileType::RegFile, 0))
        }

        fn lookup(&self, _dentry: &Dentry) -> Result<Lookup, FilesystemError> {
            let file = Kstat::new(FileType::RegFile, 0);
            let inode = self
                .0
                .get_or_init(|| Inode::new(&file, Arc::new(self.clone())));
            Ok(Lookup::Found(inode.clone()))
        }
    }

    // @behavior RT-246
    #[test]
    fn a_filesystem_handing_one_inode_for_two_names_shares_it() {
        let mount = Mount::new(
            "/data",
            Filesystem::new(Arc::new(SharesInode(Arc::default())))
                .expect("the instance must stand"),
            Access::ReadWrite,
        )
        .expect("/data is a guest path");
        let root = Path::root(mount.mnt());
        let read = OpenHow {
            read: true,
            ..OpenHow::default()
        };
        let open = |name| {
            vfs::kern_path::filp_open(&Arc::default(), &root, name, &read).expect("the name opens")
        };
        let (first, second) = (open("first"), open("second"));
        let inode = |file: &vfs::File| file.path().dentry().inode().expect("the file stands");
        assert!(
            Arc::ptr_eq(&inode(&first), &inode(&second)),
            "vfs::do_file_open of two names a Filesystem hands one inode for must record that \
             inode on both, as d_splice_alias records what iget5_locked found"
        );
    }

    /// A Filesystem making a file through `create` alone, counting each open of
    /// what it made.
    #[derive(Clone, Default)]
    struct CreatesOnly {
        opened: Arc<AtomicUsize>,
    }

    struct NoOperations;

    impl FileOperations for NoOperations {}

    impl FileSystemType for CreatesOnly {
        fn fill_super(self: Arc<Self>, sb: &mut Filesystem) -> Result<(), FilesystemError> {
            vfs::simple_fill_super(sb, self)
        }
    }

    impl InodeOperations for CreatesOnly {
        fn getattr(&self, dentry: &Dentry) -> Result<Kstat, FilesystemError> {
            Ok(match dentry.parent() {
                Some(_) => Kstat::new(FileType::RegFile, 0),
                None => Kstat::new(FileType::Dir, 0),
            })
        }

        fn lookup(&self, _dentry: &Dentry) -> Result<Lookup, FilesystemError> {
            Ok(Lookup::Absent)
        }

        fn create(&self, dentry: &Dentry) -> Result<(), FilesystemError> {
            let made = Kstat::new(FileType::RegFile, 0);
            dentry.add(Some(Inode::new(&made, Arc::new(self.clone()))));
            Ok(())
        }

        fn open(
            &self,
            _dentry: &Dentry,
            _how: &OpenHow,
        ) -> Result<Box<dyn FileOperations>, FilesystemError> {
            self.opened.fetch_add(1, Ordering::SeqCst);
            Ok(Box::new(NoOperations))
        }
    }

    // @behavior RT-244
    #[test]
    fn a_filesystem_with_create_alone_opens_what_it_made() {
        let filesystem = CreatesOnly::default();
        let mount = Mount::new(
            "/data",
            Filesystem::new(Arc::new(filesystem.clone())).expect("the instance must stand"),
            Access::ReadWrite,
        )
        .expect("/data is a guest path");
        let how = OpenHow {
            write: true,
            create: true,
            ..OpenHow::default()
        };
        let file =
            vfs::kern_path::filp_open(&Arc::default(), &Path::root(mount.mnt()), "made", &how)
                .expect("a Filesystem with create makes the file");
        assert_eq!(
            (
                file.getattr().map(|made| made.file_type),
                filesystem.opened.load(Ordering::SeqCst)
            ),
            (Ok(FileType::RegFile), 1),
            "vfs::do_file_open making a file on a Filesystem with no atomic_open must open \
             what create recorded, as lookup_open does"
        );
    }

    /// A security module refusing to open what an open made.
    struct RefusesWhatWasMade;

    impl SecurityOperations for RefusesWhatWasMade {
        fn file_open(
            &self,
            _path: SecurityPath<'_>,
            _kind: FileType,
            mode: FMode,
            _how: &OpenHow,
        ) -> Result<FileSecurity, FilesystemError> {
            match mode.contains(FMode::CREATED) {
                true => Err(FilesystemError::PermissionDenied),
                false => Ok(None),
            }
        }
    }

    // @behavior RT-288
    #[test]
    fn a_file_made_by_create_alone_opens_only_once_the_modules_allow_it() {
        let filesystem = CreatesOnly::default();
        let mount = Mount::new(
            "/data",
            Filesystem::new(Arc::new(filesystem.clone())).expect("the instance must stand"),
            Access::ReadWrite,
        )
        .expect("/data is a guest path");
        let how = OpenHow {
            write: true,
            create: true,
            ..OpenHow::default()
        };
        let cred = Cred::new(vec![Arc::new(RefusesWhatWasMade)]);
        let answer = vfs::kern_path::filp_open(&cred, &Path::root(mount.mnt()), "made", &how);
        assert_eq!(
            (answer.err(), filesystem.opened.load(Ordering::SeqCst)),
            (Some(FilesystemError::PermissionDenied), 0),
            "vfs::do_file_open making a file on a Filesystem with no atomic_open, under a \
             security module refusing the open, must never ask the Filesystem's open, as \
             do_open asks security_file_open before vfs_open reaches f_op->open"
        );
    }

    /// A Filesystem whose `open` of what it made takes the lock of the directory
    /// it made it in, as one reaching that directory again would.
    #[derive(Clone, Default)]
    struct OpensWithinItsDirectory(CreatesOnly);

    impl FileSystemType for OpensWithinItsDirectory {
        fn fill_super(self: Arc<Self>, sb: &mut Filesystem) -> Result<(), FilesystemError> {
            vfs::simple_fill_super(sb, self)
        }
    }

    impl InodeOperations for OpensWithinItsDirectory {
        fn getattr(&self, dentry: &Dentry) -> Result<Kstat, FilesystemError> {
            self.0.getattr(dentry)
        }

        fn lookup(&self, dentry: &Dentry) -> Result<Lookup, FilesystemError> {
            self.0.lookup(dentry)
        }

        fn create(&self, dentry: &Dentry) -> Result<(), FilesystemError> {
            let made = Kstat::new(FileType::RegFile, 0);
            dentry.add(Some(Inode::new(&made, Arc::new(self.clone()))));
            Ok(())
        }

        fn open(
            &self,
            dentry: &Dentry,
            how: &OpenHow,
        ) -> Result<Box<dyn FileOperations>, FilesystemError> {
            let parent = dentry.parent().expect("a made file has a directory");
            let _held = parent.inode_lock()?;
            self.0.open(dentry, how)
        }
    }

    // @behavior RT-293
    #[test]
    fn a_file_an_open_made_opens_once_its_directory_is_unlocked() {
        let filesystem = OpensWithinItsDirectory::default();
        let mount = Mount::new(
            "/data",
            Filesystem::new(Arc::new(filesystem)).expect("the instance must stand"),
            Access::ReadWrite,
        )
        .expect("/data is a guest path");
        let how = OpenHow {
            write: true,
            create: true,
            ..OpenHow::default()
        };
        assert!(
            vfs::kern_path::filp_open(&Arc::default(), &Path::root(mount.mnt()), "made", &how)
                .is_ok(),
            "vfs::do_file_open making a file must open it once the directory's lock is gone, \
             as lookup_open unlocks the directory before do_open"
        );
    }

    /// A Filesystem whose `atomic_open` finds a file standing at the name and
    /// opens it, making nothing, as one whose backend another writer reaches.
    #[derive(Clone, Default)]
    struct FindsAFileStanding(CreatesOnly);

    impl FileSystemType for FindsAFileStanding {
        fn fill_super(self: Arc<Self>, sb: &mut Filesystem) -> Result<(), FilesystemError> {
            vfs::simple_fill_super(sb, self)
        }
    }

    impl InodeOperations for FindsAFileStanding {
        fn getattr(&self, dentry: &Dentry) -> Result<Kstat, FilesystemError> {
            self.0.getattr(dentry)
        }

        fn lookup(&self, dentry: &Dentry) -> Result<Lookup, FilesystemError> {
            self.0.lookup(dentry)
        }

        fn atomic_open(&self, dentry: &Dentry, _how: &OpenHow) -> Result<Finish, FilesystemError> {
            let found = Kstat::new(FileType::RegFile, 0);
            dentry.add(Some(Inode::new(&found, Arc::new(self.clone()))));
            Ok(Finish::Open {
                file: Box::new(NoOperations),
                created: false,
            })
        }
    }

    // @behavior RT-294
    #[test]
    fn a_file_atomic_open_found_standing_opens_as_one_that_stood() {
        let mount = Mount::new(
            "/data",
            Filesystem::new(Arc::new(FindsAFileStanding::default()))
                .expect("the instance must stand"),
            Access::ReadWrite,
        )
        .expect("/data is a guest path");
        let how = OpenHow {
            write: true,
            create: true,
            exclusive: true,
            ..OpenHow::default()
        };
        let answer =
            vfs::kern_path::filp_open(&Arc::default(), &Path::root(mount.mnt()), "found", &how);
        assert_eq!(
            answer.err(),
            Some(FilesystemError::AlreadyExists),
            "vfs::do_file_open with create and exclusive, on a Filesystem whose atomic_open \
             found the file standing, must answer EEXIST, as do_open refuses O_EXCL without \
             FMODE_CREATED"
        );
    }

    /// A test's Filesystem recording each `->permission` it is asked, refusing
    /// any change once `refuse_writes` says.
    #[derive(Default)]
    struct RecordsPermission {
        asked: Mutex<Vec<(String, Mask)>>,
        refuse_writes: AtomicBool,
    }

    impl wrap::Hook for RecordsPermission {
        fn permission(
            &self,
            _inner: &dyn InodeOperations,
            dentry: &Dentry,
            mask: Mask,
        ) -> Result<(), FilesystemError> {
            let mut asked = self.asked.lock().expect("never poisoned");
            asked.push((dentry.name(), mask));
            match self.refuse_writes.load(Ordering::SeqCst) && mask.contains(Mask::WRITE) {
                true => Err(FilesystemError::PermissionDenied),
                false => Ok(()),
            }
        }
    }

    /// A security module counting the changes it is asked about.
    struct CountsChanges(Arc<AtomicUsize>);

    impl vfs::SecurityOperations for CountsChanges {
        fn inode_permission(&self, _dentry: &Dentry, mask: Mask) -> Result<(), FilesystemError> {
            if mask.contains(Mask::WRITE) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
            Ok(())
        }
    }

    // @behavior RT-255
    #[test]
    fn a_module_is_asked_only_what_a_filesystems_permission_allowed() {
        let hook = Arc::new(RecordsPermission::default());
        let root = permission_root(&hook);
        let asked = Arc::new(AtomicUsize::new(0));
        let cred = vfs::Cred::new(vec![Arc::new(CountsChanges(asked.clone()))]);
        let how = OpenHow {
            write: true,
            create: true,
            ..OpenHow::default()
        };
        let make = |name: &str| {
            (
                vfs::filename_mkdirat(&cred, &root, &format!("{name}-dir")),
                vfs::kern_path::filp_open(&cred, &root, &format!("{name}-file"), &how).map(drop),
            )
        };
        let allowed = make("made");
        let after_allowed = asked.load(Ordering::SeqCst);
        hook.refuse_writes.store(true, Ordering::SeqCst);
        let refused = make("refused");
        let denied = || Err(FilesystemError::PermissionDenied);
        assert_eq!(
            (
                allowed,
                after_allowed,
                refused,
                asked.load(Ordering::SeqCst)
            ),
            ((Ok(()), Ok(())), 2, (denied(), denied()), 2),
            "vfs::filename_mkdirat and vfs::do_file_open making a name must ask a security \
             module once a Filesystem's permission allowed the change, as inode_permission \
             asks ->permission first and may_o_create asks it of an open"
        );
    }

    /// The root of a Tmpfs of `a/b/f` and `a/d/`, whose permission `hook` answers.
    fn permission_root(hook: &Arc<RecordsPermission>) -> Path {
        let tmpfs: Tmpfs = [("a/b/f", "f"), ("a/d/keep", "k")].into_iter().collect();
        let wrapped = Wrap::new(Arc::new(tmpfs), hook.clone());
        let mount = Mount::new(
            "/data",
            Filesystem::new(Arc::new(wrapped)).expect("the instance must stand"),
            Access::ReadWrite,
        )
        .expect("/data is a guest path");
        Path::root(mount.mnt())
    }

    // @behavior RT-242
    #[test]
    fn each_call_asks_permission_where_linux_does() {
        let hook = Arc::new(RecordsPermission::default());
        let root = permission_root(&hook);
        let cred = Arc::default();
        let asks = |call: &dyn Fn()| {
            hook.asked.lock().expect("never poisoned").clear();
            call();
            hook.asked.lock().expect("never poisoned").clone()
        };
        let name = |name: &str, mask| (name.to_string(), mask);
        let change = Mask::WRITE | Mask::EXEC;
        assert_eq!(
            asks(&|| vfs::filename_mkdirat(&cred, &root, "a/b/c").expect("mkdir lands")),
            [
                name("", Mask::EXEC),
                name("a", Mask::EXEC),
                name("b", Mask::EXEC),
                name("b", change),
            ],
            "vfs::filename_mkdirat must ask of each directory walked, then of the one \
             changed, as may_lookup and may_create_dentry do"
        );
        assert!(
            asks(&|| vfs::filename_unlinkat(&cred, &root, "a/b/f").expect("unlink lands"))
                .ends_with(&[name("b", change)]),
            "vfs::filename_unlinkat must ask of the directory it changes, as may_delete_dentry"
        );
        let read = OpenHow {
            read: true,
            directory: true,
            ..OpenHow::default()
        };
        assert!(
            asks(&|| {
                vfs::kern_path::filp_open(&cred, &root, "a/d", &read).expect("the directory opens");
            })
            .ends_with(&[name("d", Mask::OPEN | Mask::READ)]),
            "vfs::do_file_open must ask of what it opens for what the open asks, as may_open"
        );
    }

    // @behavior RT-292
    #[test]
    fn an_open_that_appends_asks_to_append() {
        let hook = Arc::new(RecordsPermission::default());
        let root = permission_root(&hook);
        let append = OpenHow {
            write: true,
            append: true,
            ..OpenHow::default()
        };
        vfs::kern_path::filp_open(&Arc::default(), &root, "a/b/f", &append)
            .expect("the file opens");
        assert!(
            hook.asked
                .lock()
                .expect("never poisoned")
                .ends_with(&[("f".to_string(), Mask::OPEN | Mask::WRITE | Mask::APPEND)]),
            "an open with append through vfs::do_file_open must ask the file to append as well \
             as to write, as build_open_flags adds MAY_APPEND"
        );
    }

    // @behavior RT-263
    #[test]
    fn a_directory_moving_to_another_parent_is_asked_to_change() {
        let hook = Arc::new(RecordsPermission::default());
        let root = permission_root(&hook);
        let cred = Arc::default();
        let asks = |from: &str, to: &str| {
            hook.asked.lock().expect("never poisoned").clear();
            vfs::filename_renameat2(&cred, &root, from, &root, to).expect("the move lands");
            hook.asked.lock().expect("never poisoned").clone()
        };
        let asked_of_d = |asked: Vec<(String, Mask)>| {
            asked
                .into_iter()
                .any(|(name, mask)| name == "d" && mask == Mask::WRITE)
        };
        assert_eq!(
            (
                asked_of_d(asks("a/d", "a/b/d")),
                asked_of_d(asks("a/b/d", "a/b/e"))
            ),
            (true, false),
            "vfs::filename_renameat2 of a directory into another parent must ask its permission \
             to write, as vfs_rename does for its `..`, and a move within one parent must not"
        );
    }

    /// A read-only directory holding a file at every name, whose permission
    /// refuses every write.
    struct ReadOnlyRefusingWrites;

    impl FileSystemType for ReadOnlyRefusingWrites {
        fn fill_super(self: Arc<Self>, sb: &mut Filesystem) -> Result<(), FilesystemError> {
            sb.set_rdonly();
            vfs::simple_fill_super(sb, self)
        }
    }

    impl InodeOperations for ReadOnlyRefusingWrites {
        fn getattr(&self, dentry: &Dentry) -> Result<Kstat, FilesystemError> {
            Ok(match dentry.parent() {
                Some(_) => Kstat::new(FileType::RegFile, 0),
                None => Kstat::new(FileType::Dir, 0),
            })
        }

        fn lookup(&self, _dentry: &Dentry) -> Result<Lookup, FilesystemError> {
            let file = Kstat::new(FileType::RegFile, 0);
            Ok(Lookup::Found(Inode::new(&file, Arc::new(Self))))
        }

        fn permission(&self, _dentry: &Dentry, mask: Mask) -> Result<(), FilesystemError> {
            match mask.contains(Mask::WRITE) {
                true => Err(FilesystemError::PermissionDenied),
                false => Ok(()),
            }
        }
    }

    // Both the read-only instance and the Filesystem's permission refuse the
    // write, so only the order tells which answers first.
    // @behavior RT-279
    #[test]
    fn a_read_only_filesystem_refuses_a_write_before_its_permission_is_asked() {
        let mount = Mount::new(
            "/data",
            Filesystem::new(Arc::new(ReadOnlyRefusingWrites)).expect("the instance must stand"),
            Access::ReadWrite,
        )
        .expect("/data is a guest path");
        let how = OpenHow {
            write: true,
            ..OpenHow::default()
        };
        assert_eq!(
            vfs::kern_path::filp_open(&Arc::default(), &Path::root(mount.mnt()), "file", &how)
                .err(),
            Some(FilesystemError::ReadOnlyFilesystem),
            "vfs::do_file_open to write a file of a read-only Filesystem whose permission refuses \
             writes must answer EROFS, as sb_permission answers before ->permission"
        );
    }

    // @behavior RT-243
    #[test]
    fn what_permission_refuses_changes_nothing() {
        let hook = Arc::new(RecordsPermission::default());
        let root = permission_root(&hook);
        let cred = Arc::default();
        hook.refuse_writes.store(true, Ordering::SeqCst);
        let made = vfs::filename_mkdirat(&cred, &root, "a/made");
        let removed = vfs::filename_unlinkat(&cred, &root, "a/b/f");
        hook.refuse_writes.store(false, Ordering::SeqCst);
        assert_eq!(
            (made, removed),
            (
                Err(FilesystemError::PermissionDenied),
                Err(FilesystemError::PermissionDenied)
            ),
            "vfs::filename_mkdirat and filename_unlinkat on a directory whose permission \
             refuses a change must answer EACCES"
        );
        assert_eq!(
            vfs::vfs_fstatat(&cred, &root, "a/made", false).err(),
            Some(FilesystemError::NotFound),
            "a refused mkdir must make nothing"
        );
        assert!(
            vfs::vfs_fstatat(&cred, &root, "a/b/f", false).is_ok(),
            "a refused unlink must leave the file"
        );
    }

    /// A link at the name `l`, moving a directory the moment the walk follows it,
    /// when the walk holds no directory's lock, and leading back where it stands.
    struct MovesWhenFollowed {
        cred: Arc<vfs::Cred>,
        root: Arc<OnceLock<Path>>,
        from: &'static str,
        to: &'static str,
    }

    impl InodeOperations for MovesWhenFollowed {
        fn getattr(&self, _dentry: &Dentry) -> Result<Kstat, FilesystemError> {
            Ok(Kstat::new(FileType::Symlink, 0))
        }

        fn get_link(&self, _dentry: &Dentry) -> Result<String, FilesystemError> {
            let root = self.root.get().expect("the mount stands by the first walk");
            vfs::filename_renameat2(&self.cred, root, self.from, root, self.to)?;
            Ok(".".to_string())
        }
    }

    struct LinkAtL(Arc<dyn InodeOperations>);

    impl wrap::Hook for LinkAtL {
        fn lookup(
            &self,
            inner: &dyn InodeOperations,
            dentry: &Dentry,
        ) -> Result<Lookup, FilesystemError> {
            if dentry.name() != "l" {
                return inner.lookup(dentry);
            }
            let link = Kstat::new(FileType::Symlink, 0);
            Ok(Lookup::Found(Inode::new(&link, self.0.clone())))
        }
    }

    /// The root of a Tmpfs of `files`, where following `l` moves `from` to `to`.
    fn moves_as_walked(
        files: &[(&'static str, &'static str)],
        from: &'static str,
        to: &'static str,
    ) -> Path {
        let tmpfs: Tmpfs = files.iter().copied().collect();
        let root = Arc::new(OnceLock::new());
        let link = MovesWhenFollowed {
            cred: Arc::default(),
            root: root.clone(),
            from,
            to,
        };
        let wrapped = Wrap::new(Arc::new(tmpfs), Arc::new(LinkAtL(Arc::new(link))));
        let mount = Mount::new(
            "/data",
            Filesystem::new(Arc::new(wrapped)).expect("the instance must stand"),
            Access::ReadWrite,
        )
        .expect("/data is a guest path");
        let path = Path::root(mount.mnt());
        root.set(path.clone()).ok().expect("set only here");
        path
    }

    // Only a link gives the walk a moment holding no lock, so a move lands between
    // two of its steps without a thread racing it.
    // @behavior RT-240
    #[test]
    fn a_dotdot_a_move_landed_before_asks_the_caller_again() {
        let root = moves_as_walked(&[("a/b/keep", "k"), ("z/c", "moved")], "a/b", "z/b");
        let cred = Arc::default();
        let during = vfs::vfs_fstatat(&cred, &root, "a/b/l/../c", false);
        // Any move in the process answers a `..` so, as `rename_lock` is one for all,
        // and tests beside this one make them.
        let mut after = Err(FilesystemError::WouldBlock);
        crate::sched::testing::wait_until("the call made again answering", || {
            after = vfs::vfs_fstatat(&cred, &root, "z/b/../c", false);
            after != Err(FilesystemError::WouldBlock)
        });
        assert_eq!(
            (during.map(|_| ()), after.map(|stat| stat.size)),
            (Err(FilesystemError::WouldBlock), Ok(5)),
            "vfs_fstatat through `..` with a move landing during the walk must answer EAGAIN, \
             as handle_dots reads r_seq, and the call made again reaches where it stands now"
        );
    }

    // @behavior RT-241
    #[test]
    fn a_walk_carried_out_from_under_its_start_answers_an_escape() {
        let root = moves_as_walked(&[("a/b/k", "k"), ("z/keep", "z")], "a/b", "z/b");
        let how = OpenHow {
            read: true,
            directory: true,
            ..OpenHow::default()
        };
        let cred = Arc::default();
        let start = vfs::kern_path::filp_open(&cred, &root, "a", &how).expect("a opens");
        let ended = vfs::vfs_fstatat(&cred, start.path(), "b/l/k", false);
        assert_eq!(
            ended.err(),
            Some(FilesystemError::Escape),
            "vfs_fstatat ending on what a move carried out from under its start must \
             answer an escape, as complete_walk asks path_is_under"
        );
    }

    // The same move carries the walk out, so only the missing name answering
    // before the walk is checked tells ENOENT from the escape.
    // @behavior RT-278
    #[test]
    fn a_walk_ending_where_nothing_stands_answers_enoent_before_where_it_stands() {
        let cred = Arc::default();
        let start = || {
            let root = moves_as_walked(&[("a/b/k", "k"), ("z/keep", "z")], "a/b", "z/b");
            let how = OpenHow {
                read: true,
                directory: true,
                ..OpenHow::default()
            };
            vfs::kern_path::filp_open(&cred, &root, "a", &how).expect("a opens")
        };
        let read = OpenHow {
            read: true,
            ..OpenHow::default()
        };
        let stat = vfs::vfs_fstatat(&cred, start().path(), "b/l/missing", false);
        let open = vfs::kern_path::filp_open(&cred, start().path(), "b/l/missing", &read);
        assert_eq!(
            (stat.err(), open.err()),
            (
                Some(FilesystemError::NotFound),
                Some(FilesystemError::NotFound)
            ),
            "vfs_fstatat and do_file_open of a name nothing stands at, through a directory a \
             move carried out from under the start, must answer ENOENT, as step_into does \
             before complete_walk"
        );
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
            }))
            .expect("the instance must stand");
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
            Filesystem::new(Arc::new(Wrap::new(Arc::new(Tmpfs::new()), counts.clone())))
                .expect("the instance must stand");
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
                vfs::kern_path::filp_open(&cred, &root, "file", &how).is_ok(),
                "the file must be made"
            );
        });
        let _held =
            vfs::kern_path::filp_open(&cred, &root, "file", &how).expect("the file must stand");
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

    impl FileSystemType for HookedTmpfs {
        fn fill_super(self: Arc<Self>, sb: &mut Filesystem) -> Result<(), FilesystemError> {
            sb.set_default_d_op(Arc::new(HookedDentry(self.stale.clone())));
            vfs::simple_fill_super(sb, self)
        }
    }

    impl InodeOperations for HookedTmpfs {
        fn getattr(&self, dentry: &Dentry) -> Result<Kstat, FilesystemError> {
            self.tmpfs.getattr(dentry)
        }

        fn lookup(&self, dentry: &Dentry) -> Result<Lookup, FilesystemError> {
            self.tmpfs.lookup(dentry)
        }

        fn atomic_open(&self, dentry: &Dentry, how: &OpenHow) -> Result<Finish, FilesystemError> {
            self.tmpfs.atomic_open(dentry, how)
        }

        fn open(
            &self,
            dentry: &Dentry,
            how: &OpenHow,
        ) -> Result<Box<dyn FileOperations>, FilesystemError> {
            self.tmpfs.open(dentry, how)
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
        fn iterate_shared(
            &mut self,
            dentry: &Dentry,
            ctx: &mut vfs::DirContext<'_>,
        ) -> Result<(), FilesystemError> {
            let entries = match folded(dentry).as_str() {
                "" => self.0.entries(),
                at => self.0.open_dir(at)?.entries(),
            }?;
            vfs::dir_emit_entries(dentry, ctx, &entries);
            Ok(())
        }
    }

    impl FileSystemType for FoldsToUpper {
        fn fill_super(self: Arc<Self>, sb: &mut Filesystem) -> Result<(), FilesystemError> {
            vfs::simple_fill_super(sb, self)
        }
    }

    impl InodeOperations for FoldsToUpper {
        fn getattr(&self, dentry: &Dentry) -> Result<Kstat, FilesystemError> {
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
            how: &OpenHow,
        ) -> Result<Box<dyn FileOperations>, FilesystemError> {
            if dentry.kind() == Ok(crate::filesystem::FileType::Dir) {
                return Ok(Box::new(FoldedDir(self.root.clone())));
            }
            self.root.open(&folded(dentry), how)
        }

        fn atomic_open(&self, dentry: &Dentry, how: &OpenHow) -> Result<Finish, FilesystemError> {
            let file = self.root.create(&folded(dentry), how)?;
            Ok(Finish::Open {
                file,
                created: true,
            })
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

    impl FileSystemType for OpensItsLinks {
        fn fill_super(self: Arc<Self>, sb: &mut Filesystem) -> Result<(), FilesystemError> {
            vfs::simple_fill_super(sb, self)
        }
    }

    impl InodeOperations for OpensItsLinks {
        fn getattr(&self, _dentry: &Dentry) -> Result<Kstat, FilesystemError> {
            Ok(Kstat::new(crate::filesystem::FileType::Symlink, 0))
        }

        fn lookup(&self, _dentry: &Dentry) -> Result<Lookup, FilesystemError> {
            let link = Kstat::new(crate::filesystem::FileType::Symlink, 0);
            let ops = Arc::new(Self(self.0.clone()));
            Ok(Lookup::Found(Inode::new(&link, ops)))
        }

        fn open(
            &self,
            _dentry: &Dentry,
            _how: &OpenHow,
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
            Mount::new(
                "/data",
                Filesystem::new(Arc::new(tmpfs)).expect("the instance must stand"),
                access,
            )
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
            Mount::new(
                "/data",
                Filesystem::new(Arc::new(tmpfs)).expect("the instance must stand"),
                Access::ReadWrite,
            )
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

    impl FileSystemType for OneEntry {
        fn fill_super(self: Arc<Self>, sb: &mut Filesystem) -> Result<(), FilesystemError> {
            vfs::simple_fill_super(sb, self)
        }
    }

    impl InodeOperations for OneEntry {
        fn getattr(&self, _dentry: &Dentry) -> Result<Kstat, FilesystemError> {
            Ok(Kstat::new(crate::filesystem::FileType::Dir, 0))
        }

        fn lookup(&self, _dentry: &Dentry) -> Result<Lookup, FilesystemError> {
            let entry = Kstat::new(crate::filesystem::FileType::RegFile, 0).with_identity(1, 2);
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
        let root = root_of(
            Filesystem::new(Arc::new(OneEntry(moves.clone()))).expect("the instance must stand"),
        );
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
        let plain = Mount::new(
            "/data",
            Filesystem::new(Arc::new(tmpfs)).expect("the instance must stand"),
            Access::ReadWrite,
        )
        .expect("/data is a guest path");
        let links = Mount::new(
            "/data",
            Filesystem::new(Arc::new(OpensItsLinks(Arc::default())))
                .expect("the instance must stand"),
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
        let root = root_of(
            Filesystem::new(Arc::new(OpensItsLinks(asked.clone())))
                .expect("the instance must stand"),
        );
        let answer = vfs::kern_path::filp_open(&Arc::default(), &root, "link", &OpenHow::default());
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
        match vfs::kern_path::filp_open(&Arc::default(), start, path, &how) {
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
        let root = root_of(Filesystem::new(Arc::new(tmpfs)).expect("the instance must stand"));
        let how = OpenHow {
            read: true,
            ..OpenHow::default()
        };
        assert_eq!(
            vfs::kern_path::filp_open(&crate::security::prepare_creds([]), &root, "w", &how).err(),
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
        let root = root_of(Filesystem::new(hooked.clone()).expect("the instance must stand"));
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
        let root = root_of(Filesystem::new(Arc::new(hooked)).expect("the instance must stand"));
        vfs::filename_mkdirat(&Arc::default(), &root, "d").expect("the directory must be made");
        let remover = {
            let root = root.clone();
            thread::spawn(move || vfs::filename_rmdir(&Arc::default(), &root, "d"))
        };
        removing
            .recv_timeout(crate::sched::testing::MAX_WAIT)
            .expect("the removal must reach the Filesystem");
        vfs::filename_mkdirat(&Arc::default(), &root, "d")
            .expect("another directory must be made in its place");
        let dir = open_dir(&root, "d");
        made.send(()).ok();
        crate::sched::testing::join("the remover", remover).expect("the removal must succeed");
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
        let mount = Mount::new(
            "/data",
            Filesystem::new(Arc::new(tmpfs)).expect("the instance must stand"),
            Access::ReadWrite,
        )
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
                vfs::kern_path::filp_open(&cred, &root, "f", &how).map(drop)
            })
        };
        reaching
            .recv_timeout(crate::sched::testing::MAX_WAIT)
            .expect("the open must reach the Policy");
        let remover = thread::spawn(move || {
            let answer = vfs::filename_unlinkat(&cred, &root, "f");
            removed.send(()).ok();
            answer
        });
        let opened = crate::sched::testing::join("the open", opener);
        crate::sched::testing::join("the removal", remover).expect("the removal must land");
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
        let root = root_of(Filesystem::new(hooked.clone()).expect("the instance must stand"));
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
        let path = root_of(Filesystem::new(Arc::new(hooked)).expect("the instance must stand"));
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
        let path = root_of(Filesystem::new(Arc::new(hooked)).expect("the instance must stand"));
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
        let root = root_of(Filesystem::new(Arc::new(hooked)).expect("the instance must stand"));
        vfs::filename_mkdirat(&Arc::default(), &root, "b").expect("the directory must be made");
        let replaced = open_dir(&root, "b");
        let mover = {
            let root = root.clone();
            thread::spawn(move || vfs::filename_renameat2(&Arc::default(), &root, "a", &root, "b"))
        };
        renaming
            .recv_timeout(crate::sched::testing::MAX_WAIT)
            .expect("the move must reach the Filesystem");
        let made = crate::sched::testing::bounded(|| {
            vfs::filename_mkdirat(&Arc::default(), &replaced, "made")
        });
        let moved = crate::sched::testing::join("the mover", mover);
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
        let root = root_of(Filesystem::new(Arc::new(hooked)).expect("the instance must stand"));
        vfs::filename_mkdirat(&Arc::default(), &root, "d").expect("the directory must be made");
        let remover = {
            let root = root.clone();
            thread::spawn(move || vfs::filename_rmdir(&Arc::default(), &root, "d"))
        };
        removing
            .recv_timeout(crate::sched::testing::MAX_WAIT)
            .expect("the removal must reach the Filesystem");
        let made =
            crate::sched::testing::bounded(|| vfs::filename_mkdirat(&Arc::default(), &root, "e"));
        crate::sched::testing::join("the remover", remover).expect("the removal must succeed");
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
        let root = root_of(Filesystem::new(Arc::new(folds)).expect("the instance must stand"));
        let mover = {
            let root = root.clone();
            thread::spawn(move || {
                vfs::filename_renameat2(&Arc::default(), &root, "dir/f", &root, "dir/g")
            })
        };
        // Bounded, so a move that never reaches the Filesystem fails the
        // assertion rather than hanging the test.
        reaching.recv_timeout(crate::sched::testing::MAX_WAIT).ok();
        let removed = crate::sched::testing::bounded(|| {
            vfs::filename_unlinkat(&Arc::default(), &root, "DIR/F")
        });
        let moved = crate::sched::testing::join("the mover", mover);
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
            Filesystem::new(Arc::new(Tmpfs::new())).expect("the instance must stand"),
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
        fn getattr(&self, dentry: &Dentry) -> Result<Kstat, FilesystemError> {
            let kind = match dentry.parent() {
                Some(_) => crate::filesystem::FileType::RegFile,
                None => crate::filesystem::FileType::Dir,
            };
            Ok(Kstat::new(kind, 0))
        }

        fn lookup(&self, _dentry: &Dentry) -> Result<Lookup, FilesystemError> {
            self.looked_up.fetch_add(1, Ordering::SeqCst);
            let file = Kstat::new(crate::filesystem::FileType::RegFile, 0);
            Ok(Lookup::Found(Inode::new(&file, Arc::new(self.clone()))))
        }
    }

    impl FileSystemType for CountsLookups {
        fn fill_super(self: Arc<Self>, sb: &mut Filesystem) -> Result<(), FilesystemError> {
            sb.set_default_d_op(self.clone());
            vfs::simple_fill_super(sb, self)
        }
    }

    /// How many lookups two stats of one name, with nothing held between, ask
    /// of a Filesystem answering `deleted`.
    fn lookups_of_two_stats(deleted: bool) -> usize {
        let looked_up = Arc::new(AtomicUsize::new(0));
        let root = root_of(
            Filesystem::new(Arc::new(CountsLookups {
                looked_up: looked_up.clone(),
                deleted,
            }))
            .expect("the instance must stand"),
        );
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

    impl FileSystemType for RecordsCalls {
        fn fill_super(self: Arc<Self>, sb: &mut Filesystem) -> Result<(), FilesystemError> {
            vfs::simple_fill_super(sb, self)
        }
    }

    impl InodeOperations for RecordsCalls {
        fn getattr(&self, _dentry: &Dentry) -> Result<Kstat, FilesystemError> {
            Ok(Kstat::new(crate::filesystem::FileType::Dir, 0))
        }

        fn lookup(&self, dentry: &Dentry) -> Result<Lookup, FilesystemError> {
            if dentry.name() == "missing" {
                return Ok(Lookup::Absent);
            }
            let file = Kstat::new(crate::filesystem::FileType::RegFile, 0);
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
            _how: &OpenHow,
        ) -> Result<Box<dyn FileOperations>, FilesystemError> {
            self.0.lock().expect("never poisoned").push("open");
            Ok(Box::new(RecordedFile))
        }

        fn atomic_open(&self, dentry: &Dentry, _how: &OpenHow) -> Result<Finish, FilesystemError> {
            self.0.lock().expect("never poisoned").push("atomic_open");
            let file = Kstat::new(crate::filesystem::FileType::RegFile, 0);
            dentry.add(Some(Inode::new(&file, Arc::new(self.clone()))));
            Ok(Finish::Open {
                file: Box::new(RecordedFile),
                created: true,
            })
        }
    }

    // @behavior RT-215
    #[test]
    fn an_open_empties_what_stood_through_setattr_once_it_is_open() {
        let calls = RecordsCalls::default();
        let root =
            root_of(Filesystem::new(Arc::new(calls.clone())).expect("the instance must stand"));
        let cred = Arc::default();
        let truncate = OpenHow {
            write: true,
            create: true,
            truncate: true,
            ..OpenHow::default()
        };
        let opened = (
            vfs::kern_path::filp_open(&cred, &root, "file", &truncate).is_ok(),
            vfs::kern_path::filp_open(&cred, &root, "missing", &truncate).is_ok(),
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

    /// A directory holding a file at every name, keeping each change `setattr`
    /// is handed.
    #[derive(Clone, Default)]
    struct KeepsChanges(Arc<Mutex<Vec<crate::filesystem::Iattr>>>);

    impl FileSystemType for KeepsChanges {
        fn fill_super(self: Arc<Self>, sb: &mut Filesystem) -> Result<(), FilesystemError> {
            vfs::simple_fill_super(sb, self)
        }
    }

    impl InodeOperations for KeepsChanges {
        fn getattr(&self, dentry: &Dentry) -> Result<Kstat, FilesystemError> {
            Ok(match dentry.parent() {
                Some(_) => Kstat::new(FileType::RegFile, 0),
                None => Kstat::new(FileType::Dir, 0),
            })
        }

        fn lookup(&self, _dentry: &Dentry) -> Result<Lookup, FilesystemError> {
            let file = Kstat::new(FileType::RegFile, 0);
            Ok(Lookup::Found(Inode::new(&file, Arc::new(self.clone()))))
        }

        fn setattr(
            &self,
            _dentry: &Dentry,
            attr: &crate::filesystem::Iattr,
        ) -> Result<(), FilesystemError> {
            self.0.lock().expect("never poisoned").push(*attr);
            Ok(())
        }
    }

    // @behavior RT-281
    #[test]
    fn a_truncate_asks_setattr_to_move_the_modification_time_too() {
        use crate::filesystem::SetTime;
        let changes = KeepsChanges::default();
        let root =
            root_of(Filesystem::new(Arc::new(changes.clone())).expect("the instance must stand"));
        let cred = Arc::default();
        let write = OpenHow {
            write: true,
            ..OpenHow::default()
        };
        let mut file =
            vfs::kern_path::filp_open(&cred, &root, "file", &write).expect("the file must open");
        file.truncate(3).expect("the truncate must land");
        let truncate = OpenHow {
            truncate: true,
            ..write
        };
        vfs::kern_path::filp_open(&cred, &root, "file", &truncate).expect("the file must open");
        let named = changes
            .0
            .lock()
            .expect("never poisoned")
            .iter()
            .map(|attr| (attr.size, attr.mtime))
            .collect::<Vec<_>>();
        assert_eq!(
            named,
            [(Some(3), Some(SetTime::Now)), (Some(0), Some(SetTime::Now))],
            "File::truncate and vfs::do_file_open with O_TRUNC must hand setattr the size and a \
             modification time of now, as do_truncate names ATTR_MTIME"
        );
    }

    /// A directory holding `f`, of `kind`, which refuses whatever `refuses` asks
    /// of `f` and of what it makes, and records whether it made anything.
    #[derive(Clone)]
    struct Holds {
        kind: crate::filesystem::FileType,
        refuses: Mask,
        made: Arc<AtomicBool>,
    }

    impl Holds {
        fn of(kind: crate::filesystem::FileType, refuses: Mask) -> Self {
            Self {
                kind,
                refuses,
                made: Arc::default(),
            }
        }
    }

    impl FileSystemType for Holds {
        fn fill_super(self: Arc<Self>, sb: &mut Filesystem) -> Result<(), FilesystemError> {
            vfs::simple_fill_super(sb, self)
        }
    }

    impl InodeOperations for Holds {
        fn getattr(&self, _dentry: &Dentry) -> Result<Kstat, FilesystemError> {
            Ok(Kstat::new(crate::filesystem::FileType::Dir, 0))
        }

        fn lookup(&self, dentry: &Dentry) -> Result<Lookup, FilesystemError> {
            if dentry.name() != "f" {
                return Ok(Lookup::Absent);
            }
            let found = Kstat::new(self.kind, 0);
            Ok(Lookup::Found(Inode::new(&found, Arc::new(self.clone()))))
        }

        fn permission(&self, dentry: &Dentry, mask: Mask) -> Result<(), FilesystemError> {
            match dentry.parent().is_some() && mask.contains(self.refuses) {
                true => Err(FilesystemError::PermissionDenied),
                false => Ok(()),
            }
        }

        fn open(
            &self,
            _dentry: &Dentry,
            _how: &OpenHow,
        ) -> Result<Box<dyn FileOperations>, FilesystemError> {
            Ok(Box::new(RecordedFile))
        }

        fn atomic_open(&self, dentry: &Dentry, _how: &OpenHow) -> Result<Finish, FilesystemError> {
            self.made.store(true, Ordering::SeqCst);
            let file = Kstat::new(crate::filesystem::FileType::RegFile, 0);
            dentry.add(Some(Inode::new(&file, Arc::new(self.clone()))));
            Ok(Finish::Open {
                file: Box::new(RecordedFile),
                created: true,
            })
        }
    }

    fn open_beneath(
        holds: Holds,
        access: Access,
        path: &str,
        how: &OpenHow,
    ) -> Option<FilesystemError> {
        let mount = Mount::new(
            "/data",
            Filesystem::new(Arc::new(holds)).expect("the instance must stand"),
            access,
        )
        .expect("/data is a guest path");
        let cred = crate::security::prepare_creds([&mount]);
        vfs::kern_path::filp_open(&cred, &Path::root(mount.mnt()), path, how).err()
    }

    // Linux's `vfs_fallocate` and `generic_fadvise` read what the file is, so a
    // FIFO a Filesystem opens like any file still answers ESPIPE. Only a module
    // letting a FIFO open reaches this, as the bundled one refuses it.
    // @behavior RT-270
    #[test]
    fn an_allocation_or_advice_on_a_fifo_answers_espipe() {
        let mount = Mount::new(
            "/data",
            Filesystem::new(Arc::new(Holds::of(
                crate::filesystem::FileType::Fifo,
                Mask::EXEC,
            )))
            .expect("the instance must stand"),
            Access::ReadWrite,
        )
        .expect("/data is a guest path");
        let write = OpenHow {
            write: true,
            ..OpenHow::default()
        };
        let mut fifo =
            vfs::kern_path::filp_open(&Arc::default(), &Path::root(mount.mnt()), "f", &write)
                .unwrap_or_else(|_| panic!("the FIFO must open"));
        assert_eq!(
            (fifo.fallocate(0, 1).err(), fifo.fadvise(0, 1).err()),
            (
                Some(FilesystemError::NotSeekable),
                Some(FilesystemError::NotSeekable)
            ),
            "File::fallocate and File::fadvise of a FIFO opened to write must answer ESPIPE"
        );
    }

    // Linux's `vfs_fallocate` allocates only for a regular file or a block
    // device. Only a module letting a device open reaches this, as the bundled
    // one refuses it.
    // @behavior RT-274
    #[test]
    fn an_allocation_on_a_character_device_answers_enodev() {
        let mount = Mount::new(
            "/data",
            Filesystem::new(Arc::new(Holds::of(
                crate::filesystem::FileType::Chrdev,
                Mask::EXEC,
            )))
            .expect("the instance must stand"),
            Access::ReadWrite,
        )
        .expect("/data is a guest path");
        let write = OpenHow {
            write: true,
            ..OpenHow::default()
        };
        let mut device =
            vfs::kern_path::filp_open(&Arc::default(), &Path::root(mount.mnt()), "f", &write)
                .unwrap_or_else(|_| panic!("the device must open"));
        assert_eq!(
            device.fallocate(0, 1).err(),
            Some(FilesystemError::NoSuchDevice),
            "File::fallocate of a character device opened to write must answer ENODEV"
        );
    }

    // Linux's `do_dentry_open` takes write access to the mount for anything but
    // a special file, so a FIFO reaches the security module, which refuses it.
    // @behavior RT-260
    #[test]
    fn a_special_file_opened_to_write_beneath_a_read_only_mount_answers_as_its_module_does() {
        let write = OpenHow {
            write: true,
            ..OpenHow::default()
        };
        let file = crate::filesystem::FileType::RegFile;
        let fifo = crate::filesystem::FileType::Fifo;
        assert_eq!(
            (
                open_beneath(Holds::of(fifo, Mask::EXEC), Access::Read, "f", &write),
                open_beneath(Holds::of(file, Mask::EXEC), Access::Read, "f", &write),
            ),
            (
                Some(FilesystemError::PermissionDenied),
                Some(FilesystemError::ReadOnlyFilesystem)
            ),
            "vfs::do_file_open to write beneath a read-only Mount must answer EACCES for a FIFO, \
             as the security module refuses it, and EROFS for a file"
        );
    }

    // Linux's `do_open` asks the mount for a truncate before `may_open` asks the
    // entry, so the entry's refusal never shows.
    // @behavior RT-261
    #[test]
    fn a_truncating_open_beneath_a_read_only_mount_answers_erofs_first() {
        let truncate = OpenHow {
            read: true,
            truncate: true,
            ..OpenHow::default()
        };
        let file = crate::filesystem::FileType::RegFile;
        assert_eq!(
            open_beneath(Holds::of(file, Mask::WRITE), Access::Read, "f", &truncate),
            Some(FilesystemError::ReadOnlyFilesystem),
            "vfs::do_file_open with O_TRUNC of a file its Filesystem refuses to write, beneath a \
             read-only Mount, must answer EROFS"
        );
    }

    // @behavior RT-262
    #[test]
    fn an_open_asks_to_open_what_it_just_made() {
        let holds = Holds::of(crate::filesystem::FileType::RegFile, Mask::OPEN);
        let made = holds.made.clone();
        let create = OpenHow {
            write: true,
            create: true,
            ..OpenHow::default()
        };
        assert_eq!(
            (
                open_beneath(holds, Access::ReadWrite, "new", &create),
                made.load(Ordering::SeqCst)
            ),
            (Some(FilesystemError::PermissionDenied), true),
            "vfs::do_file_open with O_CREAT of a missing path whose Filesystem refuses to open what \
             it makes must make the file and answer EACCES"
        );
    }

    /// A directory whose lookup of `x` counts itself and holds until told to go on.
    struct HoldsLookup {
        asked: AtomicUsize,
        reached: Mutex<mpsc::Sender<()>>,
        go_on: Mutex<mpsc::Receiver<()>>,
    }

    impl FileSystemType for HoldsLookup {
        fn fill_super(self: Arc<Self>, sb: &mut Filesystem) -> Result<(), FilesystemError> {
            vfs::simple_fill_super(sb, self)
        }
    }

    impl InodeOperations for HoldsLookup {
        fn getattr(&self, _dentry: &Dentry) -> Result<Kstat, FilesystemError> {
            Ok(Kstat::new(crate::filesystem::FileType::Dir, 0))
        }

        fn lookup(&self, _dentry: &Dentry) -> Result<Lookup, FilesystemError> {
            self.asked.fetch_add(1, Ordering::SeqCst);
            self.reached.lock().expect("never poisoned").send(()).ok();
            self.go_on
                .lock()
                .expect("never poisoned")
                .recv_timeout(crate::sched::testing::MAX_WAIT)
                .ok();
            Ok(Lookup::Absent)
        }
    }

    // A second walk reaching the name while the first asks waits through the
    // registered Blocking, so the count shows it waited rather than asked.
    /// A directory whose lookup of `x` asks the runtime about `x` once more, as a
    /// Filesystem written in Ruby may stat through its own Mount, counting each
    /// lookup.
    #[derive(Default)]
    struct AsksItsOwnName {
        root: OnceLock<Path>,
        asked: AtomicUsize,
    }

    impl FileSystemType for AsksItsOwnName {
        fn fill_super(self: Arc<Self>, sb: &mut Filesystem) -> Result<(), FilesystemError> {
            vfs::simple_fill_super(sb, self)
        }
    }

    impl InodeOperations for AsksItsOwnName {
        fn getattr(&self, dentry: &Dentry) -> Result<Kstat, FilesystemError> {
            Ok(match dentry.parent() {
                Some(_) => Kstat::new(FileType::RegFile, 0),
                None => Kstat::new(FileType::Dir, 0),
            })
        }

        fn lookup(&self, _dentry: &Dentry) -> Result<Lookup, FilesystemError> {
            if self.asked.fetch_add(1, Ordering::SeqCst) == 0 {
                let root = self.root.get().expect("the root is set before any walk");
                vfs::vfs_fstatat(&Arc::default(), root, "x", false)?;
            }
            let file = Kstat::new(FileType::RegFile, 0);
            Ok(Lookup::Found(Inode::new(&file, Arc::new(Self::default()))))
        }
    }

    // Waiting for the lookup under way would wait on the walk making it, so only
    // asking again within it lets the walk answer; the bound turns a wait on
    // itself into a failure rather than a hang.
    // @behavior RT-282
    #[test]
    fn a_filesystem_asking_about_the_name_its_lookup_answers_is_asked_within_it() {
        let filesystem = Arc::new(AsksItsOwnName::default());
        let root = root_of(Filesystem::new(filesystem.clone()).expect("the instance must stand"));
        filesystem.root.set(root.clone()).ok();
        let answer = crate::sched::testing::bounded(|| {
            vfs::vfs_fstatat(&Arc::default(), &root, "x", false).map(|found| found.file_type)
        });
        assert_eq!(
            (answer, filesystem.asked.load(Ordering::SeqCst)),
            (Ok(FileType::RegFile), 2),
            "vfs_fstatat of a name whose lookup asks the runtime about that name must ask the \
             Filesystem again within the lookup and answer, rather than wait on itself"
        );
    }

    // @behavior RT-264
    #[test]
    fn walks_reaching_one_name_at_once_ask_its_filesystem_once() {
        let (reached, reaching) = mpsc::channel();
        let (go_on, going) = mpsc::channel();
        let holds = Arc::new(HoldsLookup {
            asked: AtomicUsize::new(0),
            reached: Mutex::new(reached),
            go_on: Mutex::new(going),
        });
        let root = root_of(Filesystem::new(holds.clone()).expect("the instance must stand"));
        let stat = |root: &Path| vfs::vfs_fstatat(&Arc::default(), root, "x", false).err();
        let first = {
            let root = root.clone();
            thread::spawn(move || stat(&root))
        };
        reaching
            .recv_timeout(crate::sched::testing::MAX_WAIT)
            .expect("the first walk must ask");
        let (counted, counting) = mpsc::channel();
        let second = {
            let root = root.clone();
            thread::spawn(move || {
                counted.send(count_waits()).ok();
                stat(&root)
            })
        };
        let waits = counting
            .recv_timeout(crate::sched::testing::MAX_WAIT)
            .expect("the second walk must start");
        crate::sched::testing::wait_until("the second walk waiting", || {
            waits.load(Ordering::SeqCst) > 0
        });
        go_on.send(()).ok();
        go_on.send(()).ok();
        let answers = (
            crate::sched::testing::join("the first walk", first),
            crate::sched::testing::join("the second walk", second),
        );
        assert_eq!(
            (answers, holds.asked.load(Ordering::SeqCst)),
            (
                (
                    Some(FilesystemError::NotFound),
                    Some(FilesystemError::NotFound)
                ),
                1
            ),
            "vfs::vfs_fstatat of one name from two walks at once must ask the Filesystem's \
             lookup once, the second waiting for the first's answer"
        );
    }

    /// A directory finding nothing at any name, keeping which walk asked.
    #[derive(Default)]
    struct RecordsAsker(Mutex<Vec<thread::ThreadId>>);

    impl FileSystemType for RecordsAsker {
        fn fill_super(self: Arc<Self>, sb: &mut Filesystem) -> Result<(), FilesystemError> {
            vfs::simple_fill_super(sb, self)
        }
    }

    impl InodeOperations for RecordsAsker {
        fn getattr(&self, _dentry: &Dentry) -> Result<Kstat, FilesystemError> {
            Ok(Kstat::new(FileType::Dir, 0))
        }

        fn lookup(&self, _dentry: &Dentry) -> Result<Lookup, FilesystemError> {
            self.0
                .lock()
                .expect("never poisoned")
                .push(thread::current().id());
            Ok(Lookup::Absent)
        }
    }

    // A walk reaching a name first would ask itself were the dentry marked only
    // once someone asks, so only marking it as it is made leaves the asking to
    // the walk that made it.
    // @behavior RT-264
    #[test]
    fn the_walk_that_makes_a_dentry_is_the_one_that_asks() {
        let filesystem = Arc::new(RecordsAsker::default());
        let root = root_of(Filesystem::new(filesystem.clone()).expect("the instance must stand"));
        let dir = root.dentry().clone();
        let made = root.mnt().sb().dentries().alloc_parallel(&dir, "x");
        let (counted, counting) = mpsc::channel();
        let other = {
            let (made, dir) = (made.clone(), dir.clone());
            thread::spawn(move || {
                counted.send(count_waits()).ok();
                made.lookup_inode(&dir).map(drop)
            })
        };
        let waits = counting
            .recv_timeout(crate::sched::testing::MAX_WAIT)
            .expect("the other walk must start");
        crate::sched::testing::wait_until("the other walk waiting", || {
            waits.load(Ordering::SeqCst) > 0
        });
        let own = made.lookup_inode(&dir).map(drop);
        let others = crate::sched::testing::join("the other walk", other);
        assert_eq!(
            (
                own,
                others,
                filesystem.0.lock().map(|asked| asked.clone()).ok()
            ),
            (Ok(()), Ok(()), Some(vec![thread::current().id()])),
            "Dentries::alloc_parallel must mark the dentry it makes in lookup for its walk, so \
             another walk reaching it waits and the Filesystem's lookup is asked once, by the \
             walk that made it, as d_alloc_parallel marks DCACHE_PAR_LOOKUP"
        );
    }

    /// A directory holding the link `e`, pointing nowhere, and the file `f`.
    struct PointsNowhere(crate::filesystem::FileType);

    impl FileSystemType for PointsNowhere {
        fn fill_super(self: Arc<Self>, sb: &mut Filesystem) -> Result<(), FilesystemError> {
            vfs::simple_fill_super(sb, self)
        }
    }

    impl InodeOperations for PointsNowhere {
        fn getattr(&self, _dentry: &Dentry) -> Result<Kstat, FilesystemError> {
            Ok(Kstat::new(self.0, 0))
        }

        fn lookup(&self, dentry: &Dentry) -> Result<Lookup, FilesystemError> {
            use crate::filesystem::FileType;
            let kind = match dentry.name().as_str() {
                "e" => FileType::Symlink,
                "f" => FileType::RegFile,
                _ => return Ok(Lookup::Absent),
            };
            let found = Kstat::new(kind, 0);
            Ok(Lookup::Found(Inode::new(&found, Arc::new(Self(kind)))))
        }

        fn get_link(&self, _dentry: &Dentry) -> Result<String, FilesystemError> {
            Ok(String::new())
        }
    }

    // Only a host's or a Ruby Filesystem's link can point nowhere, as Linux
    // refuses to make one; `pick_link` then leaves the walk where it stands.
    // @behavior RT-268
    #[test]
    fn a_link_pointing_nowhere_leaves_the_walk_where_it_stands() {
        use crate::filesystem::FileType;
        let root = root_of(
            Filesystem::new(Arc::new(PointsNowhere(FileType::Dir)))
                .expect("the instance must stand"),
        );
        let kind = |path: &str| {
            vfs::vfs_fstatat(&Arc::default(), &root, path, true).map(|found| found.file_type)
        };
        assert_eq!(
            (kind("e"), kind("e/f")),
            (Ok(FileType::Dir), Ok(FileType::RegFile)),
            "vfs::vfs_fstatat following a link that points nowhere must reach the directory \
             holding it, at the end of a path and in its middle"
        );
    }

    // Linux's waits on a directory and on a name in lookup are killable, so a
    // run whose deadline passed or whose thread was interrupted stops waiting
    // while the holder still holds.
    // @behavior RT-267
    #[test]
    fn a_wait_ends_once_its_run_is_to_stop() {
        use crate::sched::{run_task, Task};
        use std::time::Instant;

        let root =
            root_of(Filesystem::new(Arc::new(Tmpfs::new())).expect("the instance must stand"));
        let held = root.dentry().inode_lock().expect("the test holds the root");
        let past_deadline = {
            let root = root.clone();
            thread::spawn(move || {
                run_task(Some(Instant::now() + Duration::from_millis(50)), || {
                    vfs::filename_mkdirat(&Arc::default(), &root, "late")
                })
            })
        };
        let (task, tasked) = mpsc::channel();
        let interrupted = {
            let root = root.clone();
            thread::spawn(move || {
                run_task(None, || {
                    task.send(Task::current()).ok();
                    vfs::filename_mkdirat(&Arc::default(), &root, "stopped")
                })
            })
        };
        tasked
            .recv_timeout(crate::sched::testing::MAX_WAIT)
            .expect("the run must start")
            .interrupt();
        crate::sched::testing::wait_until("both waits ending", || {
            past_deadline.is_finished() && interrupted.is_finished()
        });
        drop(held);
        assert_eq!(
            (
                past_deadline.join().expect("the wait must not panic"),
                interrupted.join().expect("the wait must not panic"),
            ),
            (
                Err(FilesystemError::Interrupted),
                Err(FilesystemError::Interrupted)
            ),
            "vfs::filename_mkdirat waiting on a directory another thread holds must answer EINTR \
             once its run's deadline passes, and once its thread is interrupted"
        );
    }

    // @behavior RT-267
    #[test]
    fn a_wait_for_a_name_in_lookup_ends_once_its_run_is_to_stop() {
        use std::time::Instant;

        let (reached, reaching) = mpsc::channel();
        let (go_on, going) = mpsc::channel();
        let holds = Arc::new(HoldsLookup {
            asked: AtomicUsize::new(0),
            reached: Mutex::new(reached),
            go_on: Mutex::new(going),
        });
        let root = root_of(Filesystem::new(holds).expect("the instance must stand"));
        let first = {
            let root = root.clone();
            thread::spawn(move || vfs::vfs_fstatat(&Arc::default(), &root, "x", false).err())
        };
        reaching
            .recv_timeout(crate::sched::testing::MAX_WAIT)
            .expect("the first walk must ask");
        let deadline = Some(Instant::now() + Duration::from_millis(50));
        let second = crate::sched::run_task(deadline, || {
            vfs::vfs_fstatat(&Arc::default(), &root, "x", false).err()
        });
        go_on.send(()).ok();
        crate::sched::testing::join("the first walk", first);
        assert_eq!(
            second,
            Some(FilesystemError::Interrupted),
            "vfs::vfs_fstatat of a name another walk is looking up must answer EINTR once its \
             run's deadline passes"
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
                go_on
                    .recv_timeout(crate::sched::testing::MAX_WAIT)
                    .expect("the test says when to go on");
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
            Filesystem::new(Arc::new(Wrap::new(Arc::new(tmpfs), hook)))
                .expect("the instance must stand"),
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
            .recv_timeout(crate::sched::testing::MAX_WAIT)
            .expect("the held move must reach its Filesystem");
        let mover = {
            let (cred, root) = (cred.clone(), root.clone());
            thread::spawn(move || vfs::filename_renameat2(&cred, &root, "a/x", &root, "b/y"))
        };
        thread::sleep(Duration::from_millis(200));
        let renamed = vfs::filename_renameat2(&Arc::default(), &root, "a/x", &root, "a/moved");
        go_on.send(()).expect("the held move must be waiting");
        crate::sched::testing::join("the holder", holder)
            .unwrap_or_else(|_| panic!("the held move must land"));
        let moved = crate::sched::testing::join("the mover", mover);
        let stands = |path| vfs::vfs_fstatat(&cred, &root, path, false).is_ok();
        assert_eq!(
            (renamed, moved, stands("a/moved"), stands("b/y")),
            (Ok(()), Err(FilesystemError::NotFound), true, false),
            "vfs::filename_renameat2 of `a/x` that waited while another writer renamed it must \
             answer ENOENT and leave `a/moved`, which the Policy refuses, where it stands"
        );
    }
}
