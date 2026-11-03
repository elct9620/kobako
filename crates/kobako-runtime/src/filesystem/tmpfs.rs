//! `Tmpfs`: a Filesystem held in memory, living as long as the Host
//! App's object does.

use std::any::Any;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::SystemTime;

use super::path::{ancestors, name, normalize_seed};
use super::{
    DentryOperations, DirEntry, FileOperations, FileSystemType, FileType, Filesystem,
    FilesystemError, Iattr, InodeOperations, Kiocb, Kstat, Lookup, OpenHow, RenameFlag, SetTime,
};
use crate::vfs::{dir_emit_dots, generic_file_llseek, generic_read_dir, DirContext, Whence};
use crate::vfs::{Dentry, Identity, Inode, NAME_MAX};
use whiteout::Whiteout;

mod tmpfile;
mod whiteout;
mod xattr;

/// Files and directories held in tmpfs. Every Invocation it is handed to
/// shares it, so what one writes the next reads. What its files hold
/// together, and how many entries it holds, are bounded as a tmpfs mount
/// bounds its size and its inodes, so a guest cannot grow it past what
/// the Host App allowed. A directory opened beneath it is a Tmpfs too,
/// following that directory wherever it moves, as a tmpfs descriptor
/// follows its inode.
pub struct Tmpfs {
    tree: Arc<Tree>,
    dir: Arc<Dir>,
}

/// What every directory of one Tmpfs shares. No lock spans it: the VFS
/// holds each directory a call changes and serializes moves between
/// directories, as Linux's tmpfs relies on `i_rwsem` and
/// `s_vfs_rename_mutex`.
struct Tree {
    space: Arc<Space>,
    inodes_follow_limit: AtomicBool,
    /// The device number this Tmpfs reports, its own as each tmpfs mount
    /// has its own, so it shares an identity with no other Filesystem.
    dev: u64,
    /// The inode number the next directory takes, so each one made has an
    /// identity no other directory of this Tmpfs had.
    next_ino: AtomicU64,
}

#[derive(Clone)]
enum Node {
    File(Arc<Mutex<Content>>),
    Directory(Arc<Dir>),
    /// An Overlay's whiteout, as tmpfs holds a character device 0:0.
    Whiteout {
        _inode: Arc<Whiteout>,
    },
}

impl Node {
    /// Mark an entry no directory holds any more, which a handle may outlive, as
    /// `shmem_unlink` and `shmem_rmdir` drop its last links.
    fn drop_name(&self) {
        match self {
            Self::File(data) => lock(data).names = 0,
            Self::Directory(dir) => dir.state().removed = true,
            Self::Whiteout { .. } => {}
        }
    }

    /// A name of it came or went, as `simple_rename_timestamp` and
    /// `shmem_unlink` move an inode's `st_ctime`.
    fn touch_changed(&self) {
        let now = SystemTime::now();
        match self {
            Self::File(data) => lock(data).times.ctime = now,
            Self::Directory(dir) => dir.state().times.ctime = now,
            Self::Whiteout { .. } => {}
        }
    }

    fn file_type(&self) -> FileType {
        match self {
            Self::File(_) => FileType::RegFile,
            Self::Directory(_) => FileType::Dir,
            Self::Whiteout { .. } => FileType::Chrdev,
        }
    }

    /// Its entry in a listing of a Tmpfs on `dev`, named as a stat of it is.
    fn dir_entry(&self, name: &str, dev: u64) -> DirEntry {
        let entry = DirEntry::new(name, self.file_type());
        match self {
            Self::File(content) => {
                let content = lock(content);
                entry.with_identity(content.dev, content.ino)
            }
            Self::Directory(dir) => entry.with_identity(dev, dir.ino),
            Self::Whiteout { .. } => entry,
        }
    }
}

/// A directory and its inode number. Each entry is held under its own
/// name in its directory, as a tmpfs holds one name to each entry, so the
/// names it holds stay bounded by its inodes however its directories move.
struct Dir {
    ino: u64,
    /// What it counts against for as long as anything holds it, as tmpfs frees
    /// an inode when the last reference to it goes.
    space: Arc<Space>,
    state: Mutex<State>,
}

struct State {
    entries: BTreeMap<String, Node>,
    /// Each name by its stable position in a listing, as `simple_offset_ctx`
    /// keeps one, so a listing going on reads the directory as it stands without
    /// holding what it began with.
    offsets: BTreeMap<u64, String>,
    offset_of: BTreeMap<String, u64>,
    next_offset: u64,
    /// How many of `entries` are directories, as `inc_nlink` keeps a
    /// directory's count, so no stat reads every entry.
    subdirectories: u64,
    /// Whether no directory holds it any more, so it counts no name.
    removed: bool,
    /// The directory's extended attributes, as tmpfs keeps the `trusted.*`
    /// ones an Overlay marks a directory opaque with.
    xattrs: BTreeMap<String, Vec<u8>>,
    times: Times,
}

impl State {
    /// Hold `node` under `name`, answering what stood there. The name takes the
    /// next position in a listing, as `simple_offset_add` hands one out.
    fn hold(&mut self, name: &str, node: Node) -> Option<Node> {
        self.subdirectories += u64::from(matches!(node, Node::Directory(_)));
        let replaced = self.entries.insert(name.to_string(), node);
        self.subdirectories -= u64::from(matches!(replaced, Some(Node::Directory(_))));
        self.forget_offset(name);
        self.offsets.insert(self.next_offset, name.to_string());
        self.offset_of.insert(name.to_string(), self.next_offset);
        self.next_offset += 1;
        replaced
    }

    /// Leave `name` out of a listing from now on, as `simple_offset_remove`.
    fn forget_offset(&mut self, name: &str) {
        if let Some(offset) = self.offset_of.remove(name) {
            self.offsets.remove(&offset);
        }
    }

    /// The first entry a listing at `pos` reaches, and the position it stands at.
    fn entry_from(&self, pos: u64) -> Option<(u64, &str, &Node)> {
        let (offset, name) = self.offsets.range(pos..).next()?;
        Some((*offset, name, self.entries.get(name)?))
    }

    /// Hold `node` under `name`, a change to the directory's content.
    fn insert(&mut self, name: &str, node: Node) {
        node.touch_changed();
        if let Some(replaced) = self.hold(name, node) {
            replaced.drop_name();
            replaced.touch_changed();
        }
        self.times.touch();
    }

    /// Hold `node`, which keeps a name elsewhere, in place of what stands at
    /// `name`, which does too, a change to the directory's content.
    fn exchange(&mut self, name: &str, node: Node) {
        node.touch_changed();
        self.hold(name, node);
        self.times.touch();
    }

    /// Take the entry `name` away, to stand elsewhere, a change to the
    /// directory's content.
    fn take(&mut self, name: &str) {
        self.entries.remove(name).inspect(|taken| {
            self.subdirectories -= u64::from(matches!(taken, Node::Directory(_)));
        });
        self.forget_offset(name);
        self.times.touch();
    }

    /// Take the entry `name` away for good, a change to the directory's content.
    fn remove(&mut self, name: &str) {
        if let Some(removed) = self.entries.get(name).cloned() {
            removed.drop_name();
            removed.touch_changed();
        }
        self.take(name);
    }
}

/// An entry's three times, as a tmpfs mounted `noatime` keeps them: reading
/// touches nothing, a change to its content moves `st_mtime` and `st_ctime`
/// together, and a change to its names or attributes `st_ctime` alone.
#[derive(Clone, Copy)]
struct Times {
    atime: SystemTime,
    mtime: SystemTime,
    ctime: SystemTime,
}

impl Times {
    fn now() -> Self {
        let now = SystemTime::now();
        Self {
            atime: now,
            mtime: now,
            ctime: now,
        }
    }

    /// Its content changed.
    fn touch(&mut self) {
        let now = SystemTime::now();
        self.mtime = now;
        self.ctime = now;
    }

    /// Set the times `attr` names, if it names any, as `shmem_setattr` takes them
    /// from `setattr_copy`, all at the one moment the change lands, which counts
    /// as a change to the entry.
    fn apply(&mut self, attr: &Iattr) {
        if attr.atime.is_none() && attr.mtime.is_none() {
            return;
        }
        if let Some(set) = attr.atime {
            self.atime = attr.time_of(set);
        }
        if let Some(set) = attr.mtime {
            self.mtime = attr.time_of(set);
        }
        self.ctime = attr.time_of(SetTime::Now);
    }

    fn report(self, stat: Kstat) -> Kstat {
        stat.with_times(Some(self.atime), Some(self.mtime), Some(self.ctime))
    }
}

impl Dir {
    fn new(ino: u64, space: Arc<Space>) -> Arc<Self> {
        Arc::new(Self {
            ino,
            space,
            state: Mutex::new(State {
                entries: BTreeMap::new(),
                offsets: BTreeMap::new(),
                offset_of: BTreeMap::new(),
                next_offset: 2,
                subdirectories: 0,
                removed: false,
                xattrs: BTreeMap::new(),
                times: Times::now(),
            }),
        })
    }

    /// What the directory reports: two names and one more for each
    /// directory it holds, as a tmpfs counts `.` and each `..` beneath it,
    /// and a size of `BOGO_DIRENT_SIZE` for each of its entries and those
    /// two, as a tmpfs sizes a directory. One removed counts no name.
    fn getattr(&self, dev: u64) -> Kstat {
        let state = self.state();
        let size = (2 + state.entries.len() as u64) * BOGO_DIRENT_SIZE;
        let nlink = match state.removed {
            true => 0,
            false => 2 + state.subdirectories,
        };
        state.times.report(
            Kstat::new(FileType::Dir, size)
                .with_identity(dev, self.ino)
                .with_nlink(nlink),
        )
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// What stands at `name` in this directory.
    fn get(&self, name: &str) -> Result<Node, FilesystemError> {
        self.state()
            .entries
            .get(name)
            .cloned()
            .ok_or(FilesystemError::NotFound)
    }

    /// Hand `nested` every directory this one holds, leaving it holding
    /// nothing.
    fn take_directories(&mut self, nested: &mut Vec<Arc<Dir>>) {
        let state = self
            .state
            .get_mut()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for (_, node) in std::mem::take(&mut state.entries) {
            if let Node::Directory(dir) = node {
                nested.push(dir);
            }
        }
    }
}

/// A guest can stack directories as deep as its inodes reach, so the last
/// reference to one lets go of everything beneath it in a loop, as Linux's
/// `dput` walks up in one, and no depth outruns the thread's stack.
impl Drop for Dir {
    fn drop(&mut self) {
        let mut nested = Vec::new();
        self.take_directories(&mut nested);
        while let Some(dir) = nested.pop() {
            if let Some(mut dir) = Arc::into_inner(dir) {
                dir.take_directories(&mut nested);
            }
        }
        self.space.inodes.give(1);
    }
}

/// What a Tmpfs holds: the bytes of its files, and its entries, each
/// file and directory and the root counting one as an inode does.
struct Space {
    bytes: Budget,
    inodes: Budget,
}

/// How much of one thing is used, and the most that may be.
struct Budget {
    limit: AtomicUsize,
    used: AtomicUsize,
}

impl Budget {
    fn new(limit: Option<usize>) -> Self {
        Self {
            limit: AtomicUsize::new(limit.unwrap_or(usize::MAX)),
            used: AtomicUsize::new(0),
        }
    }

    fn set_limit(&self, limit: Option<usize>) {
        self.limit
            .store(limit.unwrap_or(usize::MAX), Ordering::Relaxed);
    }

    /// Count `amount` more, refusing when it would pass the limit.
    fn take(&self, amount: usize) -> Result<(), FilesystemError> {
        let limit = self.limit.load(Ordering::Relaxed);
        self.used
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |used| {
                used.checked_add(amount).filter(|&total| total <= limit)
            })
            .map(drop)
            .map_err(|_| FilesystemError::StorageFull)
    }

    /// Count `amount` more without refusing, for what the Host App seeds.
    fn add(&self, amount: usize) {
        self.used.fetch_add(amount, Ordering::Relaxed);
    }

    fn give(&self, amount: usize) {
        self.used.fetch_sub(amount, Ordering::Relaxed);
    }
}

/// One file's bytes, counted against its Tmpfs's space, with the file's
/// inode, for as long as they live, which outlasts the file's name while
/// a handle holds them.
struct Content {
    bytes: Vec<u8>,
    space: Arc<Space>,
    times: Times,
    dev: u64,
    ino: u64,
    /// One while a directory holds it, as a Tmpfs holds no hard links.
    names: u64,
}

impl Content {
    /// A file the Host App seeds, counted but never refused.
    fn seed(bytes: Vec<u8>, tree: &Tree) -> Arc<Mutex<Self>> {
        tree.space.bytes.add(bytes.len());
        tree.space.inodes.add(1);
        Self::new(bytes, tree)
    }

    /// An empty file the guest creates, refused once no inode is left.
    fn create(tree: &Tree) -> Result<Arc<Mutex<Self>>, FilesystemError> {
        tree.space.inodes.take(1)?;
        Ok(Self::new(Vec::new(), tree))
    }

    fn new(bytes: Vec<u8>, tree: &Tree) -> Arc<Mutex<Self>> {
        Arc::new(Mutex::new(Self {
            bytes,
            space: tree.space.clone(),
            times: Times::now(),
            dev: tree.dev,
            ino: tree.next_ino.fetch_add(1, Ordering::Relaxed),
            names: 1,
        }))
    }

    fn getattr(&self) -> Kstat {
        self.times.report(
            Kstat::new(FileType::RegFile, self.bytes.len() as u64)
                .with_identity(self.dev, self.ino)
                .with_nlink(self.names),
        )
    }

    /// Resize to `len`, refusing a growth past the space left or past what
    /// can be allocated, rather than aborting the host process.
    fn resize(&mut self, len: usize) -> Result<(), FilesystemError> {
        let growth = len.saturating_sub(self.bytes.len());
        if growth > 0 {
            self.space.bytes.take(growth)?;
            if self.bytes.try_reserve_exact(growth).is_err() {
                self.space.bytes.give(growth);
                return Err(FilesystemError::StorageFull);
            }
        }
        self.space.bytes.give(self.bytes.len().saturating_sub(len));
        self.bytes.resize(len, 0);
        self.bytes.shrink_to(len);
        Ok(())
    }
}

impl Drop for Content {
    fn drop(&mut self) {
        self.space.bytes.give(self.bytes.len());
        self.space.inodes.give(1);
    }
}

/// The inodes a limit of `bytes` allows unless given its own: one for
/// each 4 KiB page the bytes span, as a tmpfs counts one page to each
/// file, and one for the root.
fn inodes_for(bytes: Option<usize>) -> Option<usize> {
    bytes.map(|bytes| bytes.div_ceil(PAGE).saturating_add(1))
}

const PAGE: usize = 4096;

/// The size a tmpfs counts for each name a directory holds.
const BOGO_DIRENT_SIZE: u64 = 20;

impl Tmpfs {
    /// The most bytes a Tmpfs's files hold unless given a limit of their
    /// own: the size Docker gives a `--tmpfs` mount.
    pub const DEFAULT_SIZE: usize = 64 * 1024 * 1024;

    pub fn new() -> Self {
        let space = Space {
            bytes: Budget::new(Some(Self::DEFAULT_SIZE)),
            inodes: Budget::new(inodes_for(Some(Self::DEFAULT_SIZE))),
        };
        space.inodes.add(1);
        let space = Arc::new(space);
        Self {
            tree: Arc::new(Tree {
                space: space.clone(),
                inodes_follow_limit: AtomicBool::new(true),
                dev: Kstat::unique_device(),
                next_ino: AtomicU64::new(2),
            }),
            dir: Dir::new(1, space),
        }
    }

    /// A directory with an inode number no directory of this Tmpfs had.
    fn new_directory(&self) -> Arc<Dir> {
        Dir::new(
            self.tree.next_ino.fetch_add(1, Ordering::Relaxed),
            self.tree.space.clone(),
        )
    }

    /// This Tmpfs with its files holding at most `size` bytes together,
    /// or without bound for `None`, as a tmpfs mount's `size` does. Unless
    /// given `nr_inodes` of its own, its entries follow the size: one for
    /// each 4 KiB it spans, and the root. What it was seeded with counts,
    /// but is never refused.
    pub fn with_size(self, size: Option<usize>) -> Self {
        self.tree.space.bytes.set_limit(size);
        if self.tree.inodes_follow_limit.load(Ordering::Relaxed) {
            self.tree.space.inodes.set_limit(inodes_for(size));
        }
        self
    }

    /// This Tmpfs holding at most `nr_inodes` entries, the root among them,
    /// or without bound for `None`, as a tmpfs mount's `nr_inodes` does.
    pub fn with_nr_inodes(self, nr_inodes: Option<usize>) -> Self {
        self.tree.space.inodes.set_limit(nr_inodes);
        self.tree
            .inodes_follow_limit
            .store(false, Ordering::Relaxed);
        self
    }

    /// The directory `dir` of this Tmpfs, as a Tmpfs of its own.
    fn beneath(&self, dir: Arc<Dir>) -> Self {
        Self {
            tree: self.tree.clone(),
            dir,
        }
    }
}

impl Default for Tmpfs {
    fn default() -> Self {
        Self::new()
    }
}

/// Why a list of files cannot seed a Tmpfs as given.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SeedError {
    /// A path naming the root, where no file can stand.
    Root,
    /// A file given twice.
    Twice(String),
    /// A path given as a file where another path needs a directory, or
    /// the reverse.
    FileAndDirectory(String),
    /// A path naming `..`, which no runtime hands a Filesystem, so the
    /// file would never be reached.
    DotDot(String),
}

impl std::fmt::Display for SeedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Root => write!(f, "a Tmpfs seeds no file at its root"),
            Self::Twice(path) => write!(f, "a Tmpfs seeds {path} twice"),
            Self::FileAndDirectory(path) => {
                write!(f, "a Tmpfs seeds {path} as both a file and a directory")
            }
            Self::DotDot(path) => write!(f, "a Tmpfs seeds no path through `..`, got {path}"),
        }
    }
}

impl std::error::Error for SeedError {}

impl Tmpfs {
    /// A Tmpfs seeded with files, each `(path, contents)`, its directories
    /// created along the way, or why no Tmpfs holds the list as given.
    pub fn try_from_iter<P, C, I>(files: I) -> Result<Self, SeedError>
    where
        P: AsRef<str>,
        C: Into<Vec<u8>>,
        I: IntoIterator<Item = (P, C)>,
    {
        let tmpfs = Self::new();
        for (path, contents) in files {
            let path = normalize_seed(path.as_ref());
            if path.is_empty() {
                return Err(SeedError::Root);
            }
            if path.split('/').any(|name| name == "..") {
                return Err(SeedError::DotDot(path));
            }
            let mut dir = tmpfs.dir.clone();
            for above in ancestors(&path) {
                let standing = dir.state().entries.get(name(above)).cloned();
                let next = standing.unwrap_or_else(|| {
                    tmpfs.tree.space.inodes.add(1);
                    let made = Node::Directory(tmpfs.new_directory());
                    dir.state().hold(name(above), made.clone());
                    made
                });
                dir = match next {
                    Node::Directory(inner) => inner,
                    Node::File(_) | Node::Whiteout { .. } => {
                        return Err(SeedError::FileAndDirectory(above.to_string()))
                    }
                };
            }
            let mut state = dir.state();
            match state.entries.get(name(&path)) {
                Some(Node::Directory(_)) => return Err(SeedError::FileAndDirectory(path)),
                Some(Node::File(_) | Node::Whiteout { .. }) => return Err(SeedError::Twice(path)),
                None => {}
            }
            state.hold(
                name(&path),
                Node::File(Content::seed(contents.into(), &tmpfs.tree)),
            );
        }
        Ok(tmpfs)
    }
}

/// A Tmpfs seeded with files, each `(path, contents)`, its directories
/// created along the way.
///
/// # Panics
///
/// When `Tmpfs::try_from_iter` refuses the list: a list written into the
/// Host App's own code that no Tmpfs holds is a defect in that code.
impl<P: AsRef<str>, C: Into<Vec<u8>>> FromIterator<(P, C)> for Tmpfs {
    fn from_iter<I: IntoIterator<Item = (P, C)>>(files: I) -> Self {
        Self::try_from_iter(files).unwrap_or_else(|error| panic!("{error}"))
    }
}

impl Tmpfs {
    /// What `node`, an entry of this Tmpfs, reports of itself. A whiteout is
    /// a character device 0:0, as tmpfs makes one.
    fn node_getattr(&self, node: &Node) -> Kstat {
        match node {
            Node::File(data) => lock(data).getattr(),
            Node::Directory(dir) => dir.getattr(self.tree.dev),
            Node::Whiteout { .. } => Kstat::new(FileType::Chrdev, 0),
        }
    }

    /// The inode answering for `node`, an entry of this Tmpfs.
    fn inode_of(&self, node: Node) -> Arc<Inode> {
        let stat = self.node_getattr(&node);
        let ops: Arc<dyn InodeOperations> = match node {
            Node::Directory(dir) => Arc::new(self.beneath(dir)),
            Node::File(data) => Arc::new(TmpfsRegular { data }),
            Node::Whiteout { .. } => Arc::new(TmpfsWhiteout),
        };
        Inode::new(&stat, ops)
    }

    /// Put `node` at the name `dentry` carries and record it there, as
    /// `shmem_mknod` calls `d_instantiate`.
    fn make(&self, dentry: &Dentry, node: Node) -> Result<(), FilesystemError> {
        self.dir.state().insert(&dentry.name(), node.clone());
        dentry.add(Some(self.inode_of(node)));
        Ok(())
    }
}

impl FileSystemType for Tmpfs {
    fn fill_super(self: Arc<Self>, sb: &mut Filesystem) -> Result<(), FilesystemError> {
        sb.set_default_d_op(Arc::new(TmpfsDentry));
        sb.set_dontcache();
        sb.d_make_root(Inode::new(&self.dir.getattr(self.tree.dev), self));
        Ok(())
    }
}

/// A directory of a Tmpfs answers for the names in it. A file answers
/// through `TmpfsRegular`, and a whiteout, a character device 0:0 with no
/// driver to open it, through `TmpfsWhiteout`.
impl InodeOperations for Tmpfs {
    fn getattr(&self, _dentry: &Dentry) -> Result<Kstat, FilesystemError> {
        Ok(self.dir.getattr(self.tree.dev))
    }

    fn setattr(&self, _dentry: &Dentry, attr: &Iattr) -> Result<(), FilesystemError> {
        self.dir.state().times.apply(attr);
        Ok(())
    }

    /// A Tmpfs matches names byte for byte and holds no links, and refuses a
    /// name past `NAME_MAX`, as `simple_lookup` does.
    fn lookup(&self, dentry: &Dentry) -> Result<Lookup, FilesystemError> {
        let name = dentry.name();
        if name.len() > NAME_MAX {
            return Err(FilesystemError::InvalidFilename);
        }
        let node = match self.dir.get(&name) {
            Ok(node) => node,
            Err(FilesystemError::NotFound) => return Ok(Lookup::Absent),
            Err(err) => return Err(err),
        };
        Ok(Lookup::Found(self.inode_of(node)))
    }

    fn create(&self, dentry: &Dentry) -> Result<(), FilesystemError> {
        let data = Content::create(&self.tree)?;
        self.make(dentry, Node::File(data))
    }

    fn open(
        &self,
        _dentry: &Dentry,
        _how: &OpenHow,
    ) -> Result<Box<dyn FileOperations>, FilesystemError> {
        Ok(Box::new(TmpfsDir {
            dir: self.dir.clone(),
            dev: self.tree.dev,
        }))
    }

    fn mkdir(&self, dentry: &Dentry) -> Result<(), FilesystemError> {
        self.tree.space.inodes.take(1)?;
        self.make(dentry, Node::Directory(self.new_directory()))
    }

    /// The VFS found a directory at the name, as `shmem_rmdir` leaves it to,
    /// so only whether it holds anything is left to answer.
    fn rmdir(&self, dentry: &Dentry) -> Result<(), FilesystemError> {
        let name = dentry.name();
        let mut state = self.dir.state();
        if let Some(Node::Directory(removed)) = state.entries.get(&name) {
            if !removed.state().entries.is_empty() {
                return Err(FilesystemError::DirectoryNotEmpty);
            }
        }
        state.remove(&name);
        Ok(())
    }

    /// The VFS found no directory at the name, as `shmem_unlink` leaves it to.
    fn unlink(&self, dentry: &Dentry) -> Result<(), FilesystemError> {
        self.dir.state().remove(&dentry.name());
        Ok(())
    }

    fn rename(
        &self,
        from: &Dentry,
        to_dir: &dyn InodeOperations,
        to: &Dentry,
        flag: Option<RenameFlag>,
    ) -> Result<(), FilesystemError> {
        let target = (to_dir as &dyn Any)
            .downcast_ref::<Self>()
            .ok_or(FilesystemError::CrossesDevices)?;
        self.rename_at(&from.name(), target, &to.name(), flag)
    }
}

/// A directory of a Tmpfs, open.
struct TmpfsDir {
    dir: Arc<Dir>,
    dev: u64,
}

impl FileOperations for TmpfsDir {
    fn llseek(
        &mut self,
        dentry: &Dentry,
        pos: u64,
        offset: i64,
        whence: Whence,
    ) -> Result<u64, FilesystemError> {
        generic_file_llseek(dentry, pos, offset, whence)
    }

    fn read_iter(&mut self, _iocb: &mut Kiocb, _buf: &mut [u8]) -> Result<usize, FilesystemError> {
        generic_read_dir()
    }

    /// Reads on from each name's stable position, as `offset_readdir` does, so
    /// nothing is held between two calls.
    fn iterate_shared(
        &mut self,
        dentry: &Dentry,
        ctx: &mut DirContext<'_>,
    ) -> Result<(), FilesystemError> {
        if !dir_emit_dots(dentry, ctx) {
            return Ok(());
        }
        loop {
            let found = {
                let state = self.dir.state();
                state
                    .entry_from(ctx.pos())
                    .map(|(offset, name, node)| (offset, node.dir_entry(name, self.dev)))
            };
            let Some((offset, entry)) = found else {
                return Ok(());
            };
            ctx.set_pos(offset);
            if !ctx.dir_emit(&entry) {
                return Ok(());
            }
            ctx.set_pos(offset + 1);
        }
    }

    /// A Tmpfs holds nothing to make durable, as tmpfs names `noop_fsync` for
    /// its directories too.
    fn fsync(&mut self, _datasync: bool) -> Result<(), FilesystemError> {
        Ok(())
    }
}

/// A Tmpfs handed to more than one Filesystem instance changes through each,
/// behind the others' dentries, so a dentry recording an identity is valid
/// only while that entry stands at its name here, as NFS compares the handle
/// a fresh lookup finds. One recording none has nothing to compare. Moves
/// through two instances take no common lock, so together they are undefined.
pub(super) struct TmpfsDentry;

impl DentryOperations for TmpfsDentry {
    fn revalidate(
        &self,
        dir: &dyn InodeOperations,
        dentry: &Dentry,
    ) -> Result<bool, FilesystemError> {
        let Some(dir) = (dir as &dyn Any).downcast_ref::<Tmpfs>() else {
            return Ok(true);
        };
        let found = dir.dir.get(&dentry.name()).ok();
        let Some(inode) = dentry.inode() else {
            return Ok(found.is_none());
        };
        let Some(identity) = inode.identity() else {
            return Ok(true);
        };
        Ok(found.is_some_and(|node| Identity::of(&dir.node_getattr(&node)) == Some(identity)))
    }
}

/// A file of a Tmpfs.
struct TmpfsRegular {
    data: Arc<Mutex<Content>>,
}

impl InodeOperations for TmpfsRegular {
    fn getattr(&self, _dentry: &Dentry) -> Result<Kstat, FilesystemError> {
        Ok(lock(&self.data).getattr())
    }

    fn setattr(&self, _dentry: &Dentry, attr: &Iattr) -> Result<(), FilesystemError> {
        let mut data = lock(&self.data);
        if let Some(len) = attr.size {
            let len = usize::try_from(len).map_err(|_| FilesystemError::InvalidInput)?;
            data.resize(len)?;
        }
        data.times.apply(attr);
        Ok(())
    }

    fn open(
        &self,
        _dentry: &Dentry,
        _how: &OpenHow,
    ) -> Result<Box<dyn FileOperations>, FilesystemError> {
        Ok(Box::new(TmpfsFile {
            data: self.data.clone(),
        }))
    }
}

/// A whiteout a Tmpfs holds for an Overlay above it.
struct TmpfsWhiteout;

impl InodeOperations for TmpfsWhiteout {
    fn getattr(&self, _dentry: &Dentry) -> Result<Kstat, FilesystemError> {
        Ok(Kstat::new(FileType::Chrdev, 0))
    }
}

fn lock(data: &Mutex<Content>) -> MutexGuard<'_, Content> {
    data.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn write(data: &mut Content, buf: &[u8], start: usize) -> Result<usize, FilesystemError> {
    if buf.is_empty() {
        return Ok(0);
    }
    let end = start
        .checked_add(buf.len())
        .ok_or(FilesystemError::InvalidInput)?;
    if data.bytes.len() < end {
        data.resize(end)?;
    }
    data.bytes[start..end].copy_from_slice(buf);
    data.times.touch();
    Ok(buf.len())
}

/// An open file of a Tmpfs, sharing its bytes with every other handle on
/// the same path.
pub(crate) struct TmpfsFile {
    data: Arc<Mutex<Content>>,
}

impl FileOperations for TmpfsFile {
    fn llseek(
        &mut self,
        dentry: &Dentry,
        pos: u64,
        offset: i64,
        whence: Whence,
    ) -> Result<u64, FilesystemError> {
        generic_file_llseek(dentry, pos, offset, whence)
    }

    fn read_iter(&mut self, iocb: &mut Kiocb, buf: &mut [u8]) -> Result<usize, FilesystemError> {
        let data = &lock(&self.data).bytes;
        let start = usize::try_from(iocb.pos)
            .unwrap_or(usize::MAX)
            .min(data.len());
        let read = buf.len().min(data.len() - start);
        buf[..read].copy_from_slice(&data[start..start + read]);
        iocb.pos += read as u64;
        Ok(read)
    }

    /// The end is read and written under one lock, so another handle's
    /// append lands before or after this one, never over it.
    fn write_iter(&mut self, iocb: &mut Kiocb, buf: &[u8]) -> Result<usize, FilesystemError> {
        let mut data = lock(&self.data);
        let start = match iocb.append {
            true => data.bytes.len(),
            false => usize::try_from(iocb.pos).map_err(|_| FilesystemError::InvalidInput)?,
        };
        let written = write(&mut data, buf, start)?;
        iocb.pos = (start + written) as u64;
        drop(data);
        crate::vfs::generic_write_sync(self, iocb, written)
    }

    /// Nothing outlives the Tmpfs, so there is nothing to flush, as shmem's
    /// `noop_fsync`.
    fn fsync(&mut self, _datasync: bool) -> Result<(), FilesystemError> {
        Ok(())
    }

    /// Grow the file to where the room ends, as `shmem_fallocate` moves `i_size`
    /// past it. A Tmpfs holds every byte up to its end, so the room is taken from
    /// its size or refused as out of space.
    fn fallocate(&mut self, offset: u64, len: u64) -> Result<(), FilesystemError> {
        let end = usize::try_from(offset + len).map_err(|_| FilesystemError::StorageFull)?;
        let mut data = lock(&self.data);
        if data.bytes.len() < end {
            data.resize(end)?;
        }
        data.times.touch();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vfs::kern_path::{kern_path, KernPath};

    #[derive(Default)]
    struct CountsLookups(std::sync::atomic::AtomicUsize);

    impl crate::filesystem::wrap::Hook for CountsLookups {
        fn lookup(
            &self,
            inner: &dyn InodeOperations,
            dentry: &Dentry,
        ) -> Result<Lookup, FilesystemError> {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            inner.lookup(dentry)
        }
    }

    // @behavior FS-195
    #[test]
    fn a_listing_goes_on_from_a_position_each_name_keeps() {
        use crate::vfs::{DirContext, File, Path};
        let tmpfs: Tmpfs = [("a", ""), ("b", ""), ("c", "")].into_iter().collect();
        let mount = crate::mount::Mount::new(
            "/data",
            Filesystem::new(Arc::new(tmpfs)).expect("the instance must stand"),
            crate::mount::Access::ReadWrite,
        )
        .expect("/data is a guest path");
        let cred = Arc::default();
        let mut dir = File::root(mount.mnt(), &cred).expect("the root opens");
        let list = |dir: &mut File, pos, room: usize| {
            let mut seen = Vec::new();
            let mut actor = |emit: &crate::vfs::DirEmit<'_>| {
                let took = seen.len() < room;
                if took {
                    seen.push(emit.name.to_string());
                }
                took
            };
            let pos: u64 = pos;
            dir.llseek(pos as i64, crate::vfs::Whence::Set)
                .expect("the directory seeks");
            dir.iterate_dir(&mut DirContext::new(0, &mut actor))
                .expect("the directory lists");
            (seen, dir.pos())
        };
        let first = list(&mut dir, 2, 1);
        let root = Path::root(mount.mnt());
        crate::vfs::filename_unlinkat(&cred, &root, "a").expect("the name goes");
        crate::vfs::filename_unlinkat(&cred, &root, "b").expect("the name goes");
        let how = crate::vfs::OpenHow {
            write: true,
            create: true,
            ..crate::vfs::OpenHow::default()
        };
        crate::vfs::kern_path::filp_open(&cred, &root, "d", &how).expect("the name is made");
        let (first, next) = first;
        assert_eq!(
            (first, list(&mut dir, next, usize::MAX).0),
            (
                vec!["a".to_string()],
                vec!["c".to_string(), "d".to_string()]
            ),
            "File::iterate_dir going on over a Tmpfs must repeat nothing, pass over what was \
             removed, and hold what was made since, as offset_readdir reads on from each name's \
             stable offset"
        );
    }

    // @behavior FS-192
    #[test]
    fn a_tmpfs_keeps_no_name_nothing_holds() {
        let counts = Arc::new(CountsLookups::default());
        let tmpfs: Tmpfs = [("file", "f")].into_iter().collect();
        let wrapped = crate::filesystem::wrap::Wrap::new(Arc::new(tmpfs), counts.clone());
        let mount = crate::mount::Mount::new(
            "/data",
            Filesystem::new(Arc::new(wrapped)).expect("the instance must stand"),
            crate::mount::Access::ReadWrite,
        )
        .expect("/data is a guest path");
        let root = crate::vfs::Path::root(mount.mnt());
        for _ in 0..2 {
            crate::vfs::vfs_fstatat(&Arc::default(), &root, "file", false)
                .expect("the file stands");
        }
        assert_eq!(
            counts.0.load(std::sync::atomic::Ordering::SeqCst),
            2,
            "vfs::vfs_fstatat twice of a Tmpfs's file must ask its lookup each time, as \
             shmem sets DCACHE_DONTCACHE"
        );
    }

    // Each file leaves one name and takes another, so only keeping the count
    // through the swap reports both as still named.
    // @behavior FS-168
    #[test]
    fn an_exchange_leaves_both_files_named() {
        let filesystem = crate::filesystem::Filesystem::new(Arc::new(
            [("a", "first"), ("b", "second")]
                .into_iter()
                .collect::<Tmpfs>(),
        ))
        .expect("the instance must stand");
        let dentries = filesystem.dentries();
        let root = dentries.root();
        let (first, second) = (
            dentries.alloc_parallel(&root, "a"),
            dentries.alloc_parallel(&root, "b"),
        );
        let dir = filesystem.root().expect("the root stands").ops();
        dir.rename(&first, dir.as_ref(), &second, Some(RenameFlag::Exchange))
            .expect("the two must swap");
        let nlink = |name| {
            let dentry = dentries.alloc_parallel(&root, name);
            dentry.lookup_inode(&root).expect("the name must be found");
            dentry.getattr().map(|metadata| metadata.nlink)
        };
        assert_eq!(
            (nlink("a"), nlink("b")),
            (Ok(1), Ok(1)),
            "InodeOperations::rename of a Tmpfs asked to exchange two files must leave each with \
             one name"
        );
    }

    // Linux's VFS sets no limit on one name, so this is the Tmpfs's own.
    // @behavior FS-154
    #[test]
    fn a_name_past_its_limit_is_too_long_for_a_tmpfs() {
        let fs = kern_path(Arc::new(Tmpfs::new()));
        let how = OpenHow {
            write: true,
            ..OpenHow::default()
        };
        assert!(
            fs.create(&"x".repeat(NAME_MAX), &how).is_ok()
                && matches!(
                    fs.symlink_metadata(&"x".repeat(NAME_MAX + 1)),
                    Err(FilesystemError::InvalidFilename)
                ),
            "a name of NAME_MAX bytes must land in a Tmpfs, and one past it answer ENAMETOOLONG"
        );
    }

    // The space a Tmpfs counts is what bounds the host's memory, and a count
    // alone cannot tell, so only what the file still holds on to can.
    // @behavior FS-149
    #[test]
    fn a_file_cut_short_lets_go_of_what_it_held() {
        let tmpfs = Tmpfs::new();
        let content = Content::create(&tmpfs.tree).expect("the file must make");
        let mut data = lock(&content);
        data.resize(1 << 20).expect("the file must grow");
        data.resize(16).expect("the file must shrink");
        assert!(
            data.bytes.capacity() < 1 << 20,
            "Content::resize down to 16 bytes from 1 MiB must let go of the rest, holding {}",
            data.bytes.capacity()
        );
    }

    // @behavior FS-150
    #[test]
    fn a_file_past_its_limit_can_still_shrink() {
        let tmpfs = Tmpfs::try_from_iter([("file", vec![0; 100])])
            .expect("the seed must fit")
            .with_size(Some(10));
        let fs = kern_path(Arc::new(tmpfs));
        assert_eq!(
            fs.truncate("file", 5),
            Ok(()),
            "a truncate through Tmpfs of a file seeded past its limit must shrink it"
        );
    }

    /// `tmpfs` reached through the VFS.
    // Built directly, as a path cannot reach this deep and stacking chains
    // through the VFS takes a moving of each. A depth a recursive release
    // cannot survive on this stack is what tells it from a loop.
    // @behavior FS-145
    #[test]
    fn a_tmpfs_goes_however_deep_its_directories_stand() {
        let tmpfs = Tmpfs::new();
        let mut at = tmpfs.dir.clone();
        for _ in 0..200_000 {
            let below = tmpfs.new_directory();
            at.state()
                .entries
                .insert("a".to_string(), Node::Directory(below.clone()));
            at = below;
        }
        drop(at);
        let released = std::thread::Builder::new()
            .stack_size(64 * 1024)
            .spawn(move || drop(tmpfs))
            .expect("the thread must start")
            .join();
        assert!(
            released.is_ok(),
            "a Tmpfs 200000 directories deep, dropped on a 64 KiB stack, must go without \
             overflowing it"
        );
    }

    fn at(tmpfs: Tmpfs) -> KernPath {
        kern_path(Arc::new(tmpfs))
    }

    fn read_all(fs: &KernPath, path: &str) -> Vec<u8> {
        let mut file = fs
            .open(
                path,
                &OpenHow {
                    read: true,
                    ..OpenHow::default()
                },
            )
            .expect("the file must open");
        let mut buf = vec![0; 64];
        let read = file
            .read_iter(&mut Kiocb::at(0), &mut buf)
            .expect("the file must read");
        buf.truncate(read);
        buf
    }

    // @behavior FS-001
    #[test]
    fn a_tmpfs_serves_the_files_it_was_seeded_with() {
        let tmpfs = at([("/config/app.yml", "name: kobako")].into_iter().collect());
        assert_eq!(
            read_all(&tmpfs, "config/app.yml"),
            b"name: kobako",
            "a file seeded into Tmpfs through FromIterator must read back its content"
        );
    }

    // @behavior FS-067
    #[test]
    fn a_tmpfs_tried_with_a_list_it_cannot_hold_answers_why() {
        let refused = [
            Tmpfs::try_from_iter([("/", "x")]),
            Tmpfs::try_from_iter([("a", "x"), ("a/b", "y")]),
            Tmpfs::try_from_iter([("a", "x"), ("/a", "y")]),
        ]
        .map(|tried| tried.map(drop));
        assert_eq!(
            refused,
            [
                Err(SeedError::Root),
                Err(SeedError::FileAndDirectory("a".to_string())),
                Err(SeedError::Twice("a".to_string())),
            ],
            "a list naming the root, a file beneath a file, or a file twice through \
             Tmpfs::try_from_iter must answer the SeedError naming it"
        );
    }

    // A runtime never hands a Filesystem `..`, so a file seeded through one
    // would be held where no call reaches it.
    // @behavior FS-087
    #[test]
    fn a_tmpfs_tried_with_a_path_through_dot_dot_answers_why() {
        assert_eq!(
            Tmpfs::try_from_iter([("a/../b", "x")]).map(drop),
            Err(SeedError::DotDot("a/../b".to_string())),
            "a list naming a path through `..` through Tmpfs::try_from_iter must answer \
             SeedError::DotDot naming it"
        );
    }

    // @behavior FS-062
    #[test]
    #[should_panic(expected = "a Tmpfs seeds no file at its root")]
    fn a_tmpfs_seeded_at_its_root_panics() {
        let _: Tmpfs = [("/", "x")].into_iter().collect();
    }

    // @behavior FS-062
    #[test]
    #[should_panic(expected = "a Tmpfs seeds a as both a file and a directory")]
    fn a_tmpfs_seeded_with_a_file_beneath_a_file_panics() {
        let _: Tmpfs = [("a", "x"), ("a/b", "y")].into_iter().collect();
    }

    // @behavior FS-062
    #[test]
    #[should_panic(expected = "a Tmpfs seeds a as both a file and a directory")]
    fn a_tmpfs_seeded_with_a_file_over_a_directory_panics() {
        let _: Tmpfs = [("a/b", "y"), ("a", "x")].into_iter().collect();
    }

    // @behavior FS-062
    #[test]
    #[should_panic(expected = "a Tmpfs seeds a twice")]
    fn a_tmpfs_seeded_with_one_path_twice_panics() {
        let _: Tmpfs = [("a", "x"), ("a", "y")].into_iter().collect();
    }

    // @behavior FS-002
    #[test]
    fn a_tmpfs_keeps_what_was_written() {
        let tmpfs = at(Tmpfs::new());
        let mut file = tmpfs
            .open_or_create(
                "notes.txt",
                &OpenHow {
                    write: true,
                    ..OpenHow::default()
                },
            )
            .expect("a new file must be created");
        file.write_iter(&mut Kiocb::at(0), b"kept")
            .expect("the write must land");
        drop(file);
        assert_eq!(
            read_all(&tmpfs, "notes.txt"),
            b"kept",
            "a file written through Tmpfs::open must read back from the same Tmpfs"
        );
    }

    #[test]
    fn a_directory_with_entries_is_not_removed() {
        let tmpfs = at([("dir/file", "x")].into_iter().collect());
        assert_eq!(
            tmpfs.remove_dir("dir"),
            Err(FilesystemError::DirectoryNotEmpty)
        );
        assert_eq!(tmpfs.remove_file("dir/file"), Ok(()));
        assert_eq!(tmpfs.remove_dir("dir"), Ok(()));
        assert_eq!(
            tmpfs.symlink_metadata("dir"),
            Err(FilesystemError::NotFound)
        );
    }

    #[test]
    fn a_renamed_directory_carries_its_entries() {
        let tmpfs = at([("old/inner/file", "x")].into_iter().collect());
        tmpfs
            .rename("old", &tmpfs, "new")
            .expect("the rename must succeed");
        assert_eq!(read_all(&tmpfs, "new/inner/file"), b"x");
        assert_eq!(
            tmpfs.symlink_metadata("old"),
            Err(FilesystemError::NotFound)
        );
    }

    fn writable(fs: &KernPath, path: &str) -> Box<dyn FileOperations> {
        fs.open_or_create(
            path,
            &OpenHow {
                write: true,
                ..OpenHow::default()
            },
        )
        .expect("the file must open")
    }

    // @behavior FS-022
    #[test]
    fn a_write_past_a_tmpfs_size_is_refused() {
        let tmpfs: Tmpfs = [("file", "x")].into_iter().collect();
        let tmpfs = at(tmpfs.with_size(Some(4)));
        assert_eq!(
            (
                writable(&tmpfs, "file").write_iter(&mut Kiocb::at(1), b"four"),
                read_all(&tmpfs, "file")
            ),
            (Err(FilesystemError::StorageFull), b"x".to_vec()),
            "a write growing a Tmpfs's files past its limit through TmpfsFile::write_iter must \
             answer StorageFull and leave the file as it was"
        );
    }

    // @behavior FS-096
    #[test]
    fn a_write_of_no_bytes_leaves_a_tmpfs_file_as_it_was() {
        let tmpfs = at([("file", "")].into_iter().collect());
        assert_eq!(
            (
                writable(&tmpfs, "file").write_iter(&mut Kiocb::at(1000), b""),
                read_all(&tmpfs, "file")
            ),
            (Ok(0), Vec::new()),
            "an empty write past the end through TmpfsFile::write_iter must answer 0 and leave \
             the file empty"
        );
    }

    // @behavior FS-023
    #[test]
    fn a_tmpfs_holds_its_default_size() {
        let tmpfs = at(Tmpfs::new());
        let _file = writable(&tmpfs, "file");
        let mib_64 = 64 * 1024 * 1024;
        assert_eq!(
            (
                tmpfs.truncate("file", mib_64),
                tmpfs.truncate("file", mib_64 + 1)
            ),
            (Ok(()), Err(FilesystemError::StorageFull)),
            "a Tmpfs given no limit through Tmpfs::new must hold 64 MiB and refuse a byte more"
        );
    }

    // @behavior FS-024
    #[test]
    fn a_tmpfs_without_a_size_grows_past_the_default() {
        let tmpfs = at(Tmpfs::new().with_size(None));
        let _file = writable(&tmpfs, "file");
        assert_eq!(
            tmpfs.truncate("file", Tmpfs::DEFAULT_SIZE as u64 + 1),
            Ok(()),
            "a Tmpfs given no limit through Tmpfs::with_size(None) must grow past 64 MiB"
        );
    }

    // @behavior FS-025
    #[test]
    fn a_removed_file_returns_its_space() {
        let tmpfs = at(Tmpfs::new().with_size(Some(4)));
        writable(&tmpfs, "first")
            .write_iter(&mut Kiocb::at(0), b"four")
            .expect("the first file must fit");
        tmpfs.remove_file("first").expect("the file must remove");
        assert_eq!(
            writable(&tmpfs, "second").write_iter(&mut Kiocb::at(0), b"four"),
            Ok(4),
            "the space a file removed through Tmpfs::remove_file held must be free for the next"
        );
    }

    // A length past `isize::MAX` is one no allocator can hand out, so the
    // refusal does not depend on how much the host has free.
    // @behavior FS-026
    #[test]
    fn a_file_grown_past_what_can_be_allocated_is_refused() {
        let tmpfs: Tmpfs = [("file", "x")].into_iter().collect();
        let tmpfs = at(tmpfs.with_size(None));
        let mut file = writable(&tmpfs, "file");
        let unallocatable = isize::MAX as u64 + 1;
        assert_eq!(
            (
                tmpfs.truncate("file", unallocatable),
                file.write_iter(&mut Kiocb::at(unallocatable), b"x"),
                read_all(&tmpfs, "file")
            ),
            (
                Err(FilesystemError::StorageFull),
                Err(FilesystemError::StorageFull),
                b"x".to_vec()
            ),
            "a length no allocator can hand out through TmpfsRegular::setattr and TmpfsFile::write_iter must \
             answer StorageFull rather than abort"
        );
    }

    #[test]
    fn a_path_beneath_a_file_is_not_a_directory() {
        let tmpfs = at([("file", "x")].into_iter().collect());
        assert_eq!(
            tmpfs.symlink_metadata("file/inner"),
            Err(FilesystemError::NotADirectory)
        );
    }

    fn create(fs: &KernPath, path: &str) -> Result<(), FilesystemError> {
        fs.open_or_create(
            path,
            &OpenHow {
                write: true,
                ..OpenHow::default()
            },
        )
        .map(drop)
    }

    // Three inodes hold the root, one file, and one directory, so neither
    // kind of entry gets past the count.
    // @behavior FS-030
    #[test]
    fn a_tmpfs_holds_at_most_its_inodes() {
        let tmpfs = at(Tmpfs::new().with_nr_inodes(Some(3)));
        assert_eq!(
            (
                create(&tmpfs, "file"),
                tmpfs.create_dir("dir"),
                create(&tmpfs, "past"),
                tmpfs.create_dir("past_dir"),
            ),
            (
                Ok(()),
                Ok(()),
                Err(FilesystemError::StorageFull),
                Err(FilesystemError::StorageFull)
            ),
            "a Tmpfs given inodes through Tmpfs::with_nr_inodes must refuse a file or directory \
             past them, the root counted, as out of space"
        );
    }

    // @behavior FS-031
    #[test]
    fn a_tmpfs_inodes_follow_its_size() {
        let tmpfs = at(Tmpfs::new().with_size(Some(2 * 4096)));
        assert_eq!(
            (
                create(&tmpfs, "a"),
                create(&tmpfs, "b"),
                create(&tmpfs, "c")
            ),
            (Ok(()), Ok(()), Err(FilesystemError::StorageFull)),
            "a Tmpfs given only a limit through Tmpfs::with_size must hold one entry for each \
             4 KiB of it and the root"
        );
    }

    // The second handle opened before the first appended, so only an end
    // read as the append lands puts its bytes after the first's.
    // @behavior FS-036
    #[test]
    fn a_tmpfs_appends_at_the_end_as_it_stands() {
        let tmpfs = at([("log", "a")].into_iter().collect());
        let mut first = writable(&tmpfs, "log");
        let mut second = writable(&tmpfs, "log");
        let (mut from_first, mut from_second) =
            (Kiocb::at(0).with_append(), Kiocb::at(0).with_append());
        assert_eq!(
            (
                first.write_iter(&mut from_first, b"bb"),
                from_first.pos,
                second.write_iter(&mut from_second, b"ccc"),
                from_second.pos,
                read_all(&tmpfs, "log")
            ),
            (Ok(2), 3, Ok(3), 6, b"abbccc".to_vec()),
            "TmpfsFile::append through two handles on one file must land each at the end as it \
             stands, answering where it landed"
        );
    }

    // The VFS refuses a move onto an entry of another kind, as
    // `may_delete_dentry` does, and the Tmpfs a directory still holding
    // entries, as `shmem_rename2` does, so the two keep each move here
    // from replacing its target.
    // @behavior FS-133
    #[test]
    fn a_tmpfs_replaces_only_what_rename_lets_a_move_replace() {
        let tmpfs = at([("file", "f"), ("full/inner", "i")].into_iter().collect());
        tmpfs
            .create_dir("empty")
            .expect("the directory must be made");
        let moves = [
            ("file", "empty"),
            ("file", "full"),
            ("empty", "file"),
            ("empty", "full"),
        ]
        .map(|(from, to)| tmpfs.rename(from, &tmpfs, to).err());
        let mut standing: Vec<String> = tmpfs
            .entries()
            .expect("the root must list")
            .into_iter()
            .map(|entry| entry.name)
            .collect();
        standing.sort();
        assert_eq!(
            (moves, standing),
            (
                [
                    Some(FilesystemError::IsADirectory),
                    Some(FilesystemError::IsADirectory),
                    Some(FilesystemError::NotADirectory),
                    Some(FilesystemError::DirectoryNotEmpty),
                ],
                vec!["empty".to_string(), "file".to_string(), "full".to_string()]
            ),
            "Tmpfs::rename of a file onto a directory, and of a directory onto a file or a \
             directory holding entries, must answer EISDIR, ENOTDIR, and ENOTEMPTY, moving nothing"
        );
    }

    // A directory replaced by a rename gives its inode back as a removed
    // one does, so both leave room for the next.
    // @behavior FS-034
    #[test]
    fn a_removed_directory_gives_its_inode_back() {
        let tmpfs = at(Tmpfs::new().with_nr_inodes(Some(3)));
        tmpfs.create_dir("a").expect("the first directory must fit");
        tmpfs
            .create_dir("b")
            .expect("the second directory must fit");
        tmpfs.remove_dir("a").expect("the directory must remove");
        let after_removal = tmpfs.create_dir("c");
        tmpfs
            .rename("c", &tmpfs, "b")
            .expect("a directory must replace an empty one");
        assert_eq!(
            (after_removal, tmpfs.create_dir("d")),
            (Ok(()), Ok(())),
            "a directory removed through Tmpfs::remove_dir, or replaced through Tmpfs::rename, \
             must give its inode back"
        );
    }

    // The removed file's handle is still open, so its inode is given back
    // only when the handle goes, as Linux keeps an unlinked file's inode.
    // @behavior FS-032
    #[test]
    fn a_removed_file_gives_its_inode_back_once_closed() {
        let tmpfs = at(Tmpfs::new().with_nr_inodes(Some(2)));
        let handle = writable(&tmpfs, "first");
        tmpfs.remove_file("first").expect("the file must remove");
        let while_open = create(&tmpfs, "second");
        drop(handle);
        assert_eq!(
            (while_open, create(&tmpfs, "second")),
            (Err(FilesystemError::StorageFull), Ok(())),
            "the inode of a file removed through Tmpfs::remove_file must stay taken while a \
             handle holds it, and be free once it closes"
        );
    }

    // A host's `st_dev` never sets the top bit, so a device carrying it
    // cannot name a host's device, nor one another Filesystem took.
    // @behavior FS-085
    #[test]
    fn a_device_of_its_own_stands_apart_from_every_other_device() {
        let (first, second) = (Kstat::unique_device(), Kstat::unique_device());
        let tmpfs = at(Tmpfs::new())
            .metadata()
            .expect("the root must stand")
            .dev;
        assert!(
            [first, second, tmpfs].iter().all(|dev| dev >> 63 == 1)
                && first != second
                && tmpfs != first
                && tmpfs != second,
            "devices from Kstat::unique_device and a Tmpfs's root must each set the top bit \
             and differ from one another"
        );
    }

    fn names(dir: &Dir) -> Vec<String> {
        dir.state()
            .entries
            .iter()
            .flat_map(|(name, node)| {
                let beneath = match node {
                    Node::Directory(inner) => names(inner),
                    Node::File(_) | Node::Whiteout { .. } => Vec::new(),
                };
                std::iter::once(name.clone()).chain(beneath)
            })
            .collect()
    }

    // Each round nests the file one level deeper, so a Tmpfs holding
    // whole paths would hold names growing with the depth; held under its
    // own name, every entry stays the one letter it was given.
    // @behavior FS-094
    #[test]
    fn a_tmpfs_holds_each_entry_under_its_own_name() {
        let held: Arc<Tmpfs> = Arc::new([("d/f", "x")].into_iter().collect());
        let tmpfs = kern_path(held.clone());
        for _ in 0..64 {
            tmpfs
                .create_dir("n")
                .expect("the new directory must be made");
            tmpfs
                .rename("d", &tmpfs, "n/d")
                .expect("the directory must move into the new one");
            tmpfs
                .rename("n", &tmpfs, "d")
                .expect("the new directory must take the old name");
        }
        let held = names(&held.dir);
        assert_eq!(
            (held, read_all(&tmpfs, &format!("{}f", "d/".repeat(65)))),
            (
                std::iter::repeat_n("d".to_string(), 65)
                    .chain(["f".to_string()])
                    .collect::<Vec<_>>(),
                b"x".to_vec()
            ),
            "a directory moved deeper 64 times through Tmpfs::rename must leave every entry \
             holding only its own name, and the file reading where it now lies"
        );
    }

    // @behavior FS-105
    #[test]
    fn a_directory_opened_from_a_tmpfs_follows_it_when_it_moves() {
        let tmpfs = at([("dir/.keep", "")].into_iter().collect());
        let dir = tmpfs.open_dir("dir").expect("the directory must open");
        tmpfs
            .rename("dir", &tmpfs, "moved")
            .expect("the directory must move");
        create(&dir, "made").expect("the file must be made");
        assert_eq!(
            (
                tmpfs.symlink_metadata("moved/made").map(|m| m.file_type),
                tmpfs.symlink_metadata("dir/made").map(|m| m.file_type)
            ),
            (Ok(FileType::RegFile), Err(FilesystemError::NotFound)),
            "a file made through a directory Tmpfs::open_dir opened before Tmpfs::rename moved \
             it must stand beneath the directory's new path"
        );
    }

    // @behavior FS-106
    #[test]
    fn a_tmpfs_directory_removed_while_opened_makes_nothing_more() {
        let tmpfs = at(Tmpfs::new());
        tmpfs.create_dir("dir").expect("the directory must be made");
        let dir = tmpfs.open_dir("dir").expect("the directory must open");
        tmpfs.remove_dir("dir").expect("the directory must remove");
        assert_eq!(
            (
                create(&dir, "made"),
                dir.create_dir("inner"),
                dir.entries().map(|entries| entries.len()),
                dir.symlink_metadata("").map(|m| m.file_type)
            ),
            (
                Err(FilesystemError::NotFound),
                Err(FilesystemError::NotFound),
                Ok(0),
                Ok(FileType::Dir)
            ),
            "a directory Tmpfs::open_dir opened, once Tmpfs::remove_dir removed it, must make \
             nothing, list nothing, and still stat as a directory"
        );
    }

    // @behavior FS-107
    #[test]
    fn a_tmpfs_directory_never_moves_beneath_itself() {
        let tmpfs = at(Tmpfs::new());
        tmpfs.create_dir("a").expect("the directory must be made");
        tmpfs
            .create_dir("a/b")
            .expect("the inner directory must be made");
        let inner = tmpfs.open_dir("a/b").expect("the directory must open");
        assert_eq!(
            (
                tmpfs.rename("a", &inner, "a"),
                tmpfs.symlink_metadata("a/b").map(|m| m.file_type)
            ),
            (Err(FilesystemError::InvalidInput), Ok(FileType::Dir)),
            "Tmpfs::rename of a directory into one opened beneath it must answer InvalidInput \
             and move nothing"
        );
    }

    // The content stands as it was, so only `st_ctime` tells the name moved, as
    // `simple_rename_timestamp` moves it alone. The wait passes the clock's
    // resolution, so the two times cannot pass for one.
    // @behavior FS-177
    #[test]
    fn a_moved_file_moves_only_its_changed_time() {
        let tmpfs = at([("file", "x")].into_iter().collect());
        let before = tmpfs.symlink_metadata("file").expect("the file must stand");
        std::thread::sleep(std::time::Duration::from_millis(20));
        tmpfs
            .rename("file", &tmpfs, "moved")
            .expect("the move must land");
        let after = tmpfs
            .symlink_metadata("moved")
            .expect("the file must stand");
        assert!(
            after.ctime > before.ctime && after.mtime == before.mtime,
            "Tmpfs::rename of a file must move its changed time and leave its modified time"
        );
    }

    // Linux's `notify_change` reads `current_time` once and `shmem_setattr` sets
    // every time from it, so a clock read for each would leave them apart. A
    // moment long past tells the one handed from one read here.
    // @behavior FS-199
    #[test]
    fn times_set_to_now_read_the_one_moment_the_change_lands() {
        let tmpfs = at([("file", "x")].into_iter().collect());
        let landed = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1);
        let now = Some(SetTime::Now);
        tmpfs
            .setattr(
                "file",
                &Iattr {
                    ctime: Some(landed),
                    ..Iattr::times(now, now)
                },
            )
            .expect("the times must be set");
        let set = tmpfs.symlink_metadata("file").expect("the file must stand");
        assert_eq!(
            (set.atime, set.mtime, set.ctime),
            (Some(landed), Some(landed), Some(landed)),
            "Tmpfs::setattr setting both times to now must set them and its changed time to the \
             moment the change lands"
        );
    }

    // tmpfs frees an inode when the last reference goes, so a directory removed
    // while it is open still counts, and only its close lets another be made.
    // @behavior FS-173
    #[test]
    fn a_removed_directory_something_holds_keeps_its_inode() {
        let mount = crate::mount::Mount::new(
            "/data",
            Filesystem::new(Arc::new(Tmpfs::new().with_nr_inodes(Some(2))))
                .expect("the instance must stand"),
            crate::mount::Access::ReadWrite,
        )
        .expect("/data is a guest path");
        let root = crate::vfs::Path::root(mount.mnt());
        let cred = Arc::default();
        crate::vfs::filename_mkdirat(&cred, &root, "held").expect("the directory must be made");
        let how = crate::vfs::OpenHow {
            read: true,
            ..Default::default()
        };
        let Ok(open) = crate::vfs::kern_path::filp_open(&cred, &root, "held", &how) else {
            panic!("the directory must open");
        };
        crate::vfs::filename_rmdir(&cred, &root, "held").expect("the directory must be removed");
        let while_open = crate::vfs::filename_mkdirat(&cred, &root, "next");
        drop(open);
        assert_eq!(
            (
                while_open,
                crate::vfs::filename_mkdirat(&cred, &root, "next")
            ),
            (Err(FilesystemError::StorageFull), Ok(())),
            "vfs::filename_mkdirat in a Tmpfs out of inodes must answer ENOSPC while a removed \
             directory stays open, and succeed once it closes"
        );
    }

    // @behavior FS-109
    #[test]
    fn a_rename_into_a_directory_of_another_tmpfs_crosses_devices() {
        let mount = |tmpfs: Tmpfs| {
            crate::mount::Mount::new(
                "/data",
                Filesystem::new(Arc::new(tmpfs)).expect("the instance must stand"),
                crate::mount::Access::ReadWrite,
            )
            .expect("/data is a guest path")
        };
        let (from, to) = (
            mount([("file", "x")].into_iter().collect()),
            mount(Tmpfs::new()),
        );
        let root = |mount: &crate::mount::Mount| crate::vfs::Path::root(mount.mnt());
        let cred = Arc::default();
        assert_eq!(
            (
                crate::vfs::filename_renameat2(&cred, &root(&from), "file", &root(&to), "file"),
                crate::vfs::vfs_fstatat(&cred, &root(&from), "file", false).map(|found| found.size),
            ),
            (Err(FilesystemError::CrossesDevices), Ok(1)),
            "vfs::filename_renameat2 into a directory of another Tmpfs must answer EXDEV and \
             leave the file"
        );
    }

    // The write waits past the clock's resolution, so a time left where
    // the file was made cannot pass for one the write moved.
    // @behavior FS-115
    #[test]
    fn a_tmpfs_keeps_a_files_times_as_a_tmpfs_mounted_noatime_does() {
        let tmpfs = at(Tmpfs::new());
        let how = OpenHow {
            read: true,
            write: true,
            ..OpenHow::default()
        };
        let mut file = tmpfs
            .open_or_create("file", &how)
            .expect("the file must be made");
        file.write_iter(&mut Kiocb::at(0), b"ab")
            .expect("the file must take a write");
        let made = tmpfs
            .symlink_metadata("file")
            .expect("the file must report");
        file.read_iter(&mut Kiocb::at(0), &mut [0; 1])
            .expect("the file must read");
        std::thread::sleep(std::time::Duration::from_millis(10));
        file.write_iter(&mut Kiocb::at(0), b"c")
            .expect("the file must take a write within its length");
        let written = tmpfs.symlink_metadata("file").expect("the file must stand");
        assert_eq!(
            (
                written.atime == made.atime,
                written.mtime > made.mtime,
                written.ctime == written.mtime,
            ),
            (true, true, true),
            "Tmpfs::metadata of a file read and then written must keep its access time from \
             when it was made, and move its modification and change times to the write"
        );
    }

    // @behavior FS-116
    #[test]
    fn a_tmpfs_counts_a_directorys_names_as_a_tmpfs_does() {
        let tmpfs = at([("dir/file", "x")].into_iter().collect());
        tmpfs.create_dir("dir/a").expect("a directory must be made");
        tmpfs.create_dir("dir/b").expect("a directory must be made");
        let nlink = |path| {
            tmpfs
                .symlink_metadata(path)
                .expect("the entry must stand")
                .nlink
        };
        assert_eq!(
            (nlink("dir"), nlink("dir/file")),
            (4, 1),
            "Tmpfs::metadata of a directory holding a file and two directories must count four \
             names, and of the file one"
        );
    }

    // A Tmpfs is the one Filesystem answering this itself, so only its own count
    // can leave a removed directory reporting names it no longer has.
    // @behavior FS-178
    #[test]
    fn a_removed_directory_something_holds_counts_no_name() {
        let tmpfs = at(Tmpfs::new());
        for path in ["gone", "replaced", "moved", "other"] {
            tmpfs.create_dir(path).expect("a directory must be made");
        }
        let held = |path| tmpfs.open_dir(path).expect("the directory must open");
        let (gone, replaced, moved) = (held("gone"), held("replaced"), held("moved"));
        tmpfs.remove_dir("gone").expect("the directory must go");
        tmpfs
            .rename("other", &tmpfs, "replaced")
            .expect("the move must replace the directory");
        tmpfs
            .rename("moved", &tmpfs, "there")
            .expect("the move must land");
        let nlink = |dir: &KernPath| dir.metadata().expect("the directory must answer").nlink;
        assert_eq!(
            (nlink(&gone), nlink(&replaced), nlink(&moved)),
            (0, 0, 2),
            "Tmpfs::metadata of a held directory removed, or replaced by a move, must count no \
             name, and of one that moved must count its two"
        );
    }

    // @behavior FS-137
    #[test]
    fn a_tmpfs_sizes_a_directory_as_a_tmpfs_does() {
        let tmpfs = at([("dir/file", "x")].into_iter().collect());
        tmpfs.create_dir("dir/a").expect("a directory must be made");
        tmpfs.create_dir("empty").expect("a directory must be made");
        let len = |path| {
            tmpfs
                .symlink_metadata(path)
                .expect("the directory must stand")
                .size
        };
        assert_eq!(
            (len("dir"), len("empty")),
            (80, 40),
            "Tmpfs::metadata of a directory holding two entries must report a size of 80, and of \
             an empty one 40"
        );
    }

    // @behavior FS-070
    #[test]
    fn a_tmpfs_gives_each_directory_it_makes_an_identity_of_its_own() {
        let tmpfs = at(Tmpfs::new());
        tmpfs.create_dir("dir").expect("the directory must be made");
        let identity = |path| {
            let metadata = tmpfs
                .symlink_metadata(path)
                .expect("the directory must stand");
            (metadata.dev, metadata.ino)
        };
        let before = identity("dir");
        tmpfs
            .rename("dir", &tmpfs, "moved")
            .expect("the directory must move");
        tmpfs
            .create_dir("dir")
            .expect("another directory must be made");
        assert_eq!(
            (identity("moved") == before, identity("dir") == before),
            (true, false),
            "a directory moved through Tmpfs::rename must keep its identity, and one made at its \
             old path through Tmpfs::create_dir must have another"
        );
    }

    mod stress {
        use std::sync::atomic::{AtomicBool, Ordering};

        use super::*;

        // A move leaves its old directory before it reaches the new one, so only
        // a stat landing between the two shows a move counted as a removal.
        // @behavior FS-201
        #[test]
        #[ignore = "a stress witness; run by rake crates:stress"]
        fn a_moved_file_keeps_its_names_through_the_move() {
            let mount = crate::mount::Mount::new(
                "/data",
                Filesystem::new(Arc::new([("a", "x")].into_iter().collect::<Tmpfs>()))
                    .expect("the instance must stand"),
                crate::mount::Access::ReadWrite,
            )
            .expect("/data is a guest path");
            let root = crate::vfs::Path::root(mount.mnt());
            let how = crate::vfs::OpenHow {
                read: true,
                ..Default::default()
            };
            let held = crate::vfs::kern_path::filp_open(&Arc::default(), &root, "a", &how)
                .unwrap_or_else(|_| panic!("the file must open"));
            let stop = AtomicBool::new(false);
            let nameless = std::thread::scope(|scope| {
                scope.spawn(|| {
                    while !stop.load(Ordering::SeqCst) {
                        for (from, to) in [("a", "b"), ("b", "a")] {
                            crate::vfs::filename_renameat2(&Arc::default(), &root, from, &root, to)
                                .unwrap_or_else(|_| panic!("the move must land"));
                        }
                    }
                });
                let nameless = (0..200_000)
                    .filter(|_| held.getattr().is_ok_and(|found| found.nlink == 0))
                    .count();
                stop.store(true, Ordering::SeqCst);
                nameless
            });
            assert_eq!(
                nameless, 0,
                "File::getattr of a Tmpfs file another thread keeps moving must never report it \
                 with no name"
            );
        }
    }
}
