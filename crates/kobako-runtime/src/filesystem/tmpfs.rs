//! `Tmpfs`: a Filesystem held in memory, living as long as the Host
//! App's object does.

use std::any::Any;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::SystemTime;

use super::path::{ancestors, name, normalize_seed};
use super::{DirEntry, File, FileType, FilesystemError, Inode, Metadata, OpenOptions};
use crate::vfs::{Dentry, Identity};
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
    fn file_type(&self) -> FileType {
        match self {
            Self::File(_) => FileType::File,
            Self::Directory(_) => FileType::Directory,
            Self::Whiteout { .. } => FileType::Unknown,
        }
    }
}

/// A directory and its inode number. Each entry is held under its own
/// name in its directory, as a tmpfs holds one name to each entry, so the
/// names it holds stay bounded by its inodes however its directories move.
struct Dir {
    ino: u64,
    state: Mutex<State>,
}

struct State {
    entries: BTreeMap<String, Node>,
    /// The directory's extended attributes, as tmpfs keeps the `trusted.*`
    /// ones an Overlay marks a directory opaque with.
    xattrs: BTreeMap<String, Vec<u8>>,
    times: Times,
}

impl State {
    /// Hold `node` under `name`, a change to the directory's content.
    fn insert(&mut self, name: &str, node: Node) {
        self.entries.insert(name.to_string(), node);
        self.times.touch();
    }

    /// Take the entry `name` away, a change to the directory's content.
    fn remove(&mut self, name: &str) {
        self.entries.remove(name);
        self.times.touch();
    }
}

/// When an entry was made, and when its content last changed, as a tmpfs
/// mounted `noatime` keeps them: reading touches nothing, and a change
/// moves `st_mtime` and `st_ctime` together.
#[derive(Clone, Copy)]
struct Times {
    made: SystemTime,
    changed: SystemTime,
}

impl Times {
    fn now() -> Self {
        let now = SystemTime::now();
        Self {
            made: now,
            changed: now,
        }
    }

    fn touch(&mut self) {
        self.changed = SystemTime::now();
    }

    /// Set either time given, as `utimensat` does.
    fn set(&mut self, accessed: Option<SystemTime>, modified: Option<SystemTime>) {
        self.made = accessed.unwrap_or(self.made);
        self.changed = modified.unwrap_or(self.changed);
    }

    fn report(self, metadata: Metadata) -> Metadata {
        metadata.with_times(Some(self.made), Some(self.changed), Some(self.changed))
    }
}

impl Dir {
    fn new(ino: u64) -> Arc<Self> {
        Arc::new(Self {
            ino,
            state: Mutex::new(State {
                entries: BTreeMap::new(),
                xattrs: BTreeMap::new(),
                times: Times::now(),
            }),
        })
    }

    /// What the directory reports: two names and one more for each
    /// directory it holds, as a tmpfs counts `.` and each `..` beneath it,
    /// and a size of `BOGO_DIRENT_SIZE` for each of its entries and those
    /// two, as a tmpfs sizes a directory.
    fn metadata(&self, dev: u64) -> Metadata {
        let state = self.state();
        let subdirectories = state
            .entries
            .values()
            .filter(|node| matches!(node, Node::Directory(_)))
            .count() as u64;
        let len = (2 + state.entries.len() as u64) * BOGO_DIRENT_SIZE;
        state.times.report(
            Metadata::new(FileType::Directory, len)
                .with_identity(dev, self.ino)
                .with_nlink(2 + subdirectories),
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

    /// The directory standing at `name` in this one.
    fn directory(&self, name: &str) -> Result<Arc<Self>, FilesystemError> {
        match self.get(name)? {
            Node::Directory(dir) => Ok(dir),
            Node::File(_) | Node::Whiteout { .. } => Err(FilesystemError::NotADirectory),
        }
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
}

impl Content {
    /// A file the Host App seeds, counted but never refused.
    fn seed(bytes: Vec<u8>, space: &Arc<Space>) -> Arc<Mutex<Self>> {
        space.bytes.add(bytes.len());
        space.inodes.add(1);
        Self::new(bytes, space)
    }

    /// An empty file the guest creates, refused once no inode is left.
    fn create(space: &Arc<Space>) -> Result<Arc<Mutex<Self>>, FilesystemError> {
        space.inodes.take(1)?;
        Ok(Self::new(Vec::new(), space))
    }

    fn new(bytes: Vec<u8>, space: &Arc<Space>) -> Arc<Mutex<Self>> {
        Arc::new(Mutex::new(Self {
            bytes,
            space: space.clone(),
            times: Times::now(),
        }))
    }

    /// What the file reports, named once, as a Tmpfs holds no hard links.
    fn metadata(&self) -> Metadata {
        self.times
            .report(Metadata::new(FileType::File, self.bytes.len() as u64).with_nlink(1))
    }

    /// Resize to `len`, refusing a growth past the space left or past what
    /// can be allocated, rather than aborting the host process.
    fn resize(&mut self, len: usize) -> Result<(), FilesystemError> {
        let growth = len.saturating_sub(self.bytes.len());
        self.space.bytes.take(growth)?;
        if self.bytes.try_reserve_exact(growth).is_err() {
            self.space.bytes.give(growth);
            return Err(FilesystemError::StorageFull);
        }
        self.space.bytes.give(self.bytes.len().saturating_sub(len));
        self.bytes.resize(len, 0);
        self.times.touch();
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
        Self {
            tree: Arc::new(Tree {
                space: Arc::new(space),
                inodes_follow_limit: AtomicBool::new(true),
                dev: Metadata::unique_device(),
                next_ino: AtomicU64::new(2),
            }),
            dir: Dir::new(1),
        }
    }

    /// A directory with an inode number no directory of this Tmpfs had.
    fn new_directory(&self) -> Arc<Dir> {
        Dir::new(self.tree.next_ino.fetch_add(1, Ordering::Relaxed))
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
        let space = &tmpfs.tree.space;
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
                let next = dir
                    .state()
                    .entries
                    .entry(name(above).to_string())
                    .or_insert_with(|| {
                        space.inodes.add(1);
                        Node::Directory(tmpfs.new_directory())
                    })
                    .clone();
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
            state.entries.insert(
                name(&path).to_string(),
                Node::File(Content::seed(contents.into(), space)),
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

impl Inode for Tmpfs {
    fn open(
        &self,
        dentry: &Dentry,
        options: &OpenOptions,
    ) -> Result<Box<dyn File>, FilesystemError> {
        let name = &dentry.name();
        let data = match self.dir.get(name) {
            Ok(Node::Directory(_)) => return Err(FilesystemError::IsADirectory),
            // A character device 0:0 has no driver to open it.
            Ok(Node::Whiteout { .. }) => return Err(FilesystemError::Unsupported),
            Ok(Node::File(data)) => {
                if options.truncate {
                    lock(&data).resize(0)?;
                }
                data
            }
            Err(FilesystemError::NotFound) if options.create || options.create_new => {
                let data = Content::create(&self.tree.space)?;
                self.dir.state().insert(name, Node::File(data.clone()));
                data
            }
            Err(error) => return Err(error),
        };
        Ok(Box::new(TmpfsFile { data }))
    }

    /// A Tmpfs holds no links to refuse.
    fn open_dir(&self, dentry: &Dentry) -> Result<Arc<dyn Inode>, FilesystemError> {
        Ok(Arc::new(self.beneath(self.dir.directory(&dentry.name())?)))
    }

    fn metadata(&self, _dentry: &Dentry) -> Result<Metadata, FilesystemError> {
        Ok(self.dir.metadata(self.tree.dev))
    }

    /// A Tmpfs holds no links.
    fn symlink_metadata(&self, dentry: &Dentry) -> Result<Metadata, FilesystemError> {
        match self.dir.get(&dentry.name())? {
            Node::File(data) => Ok(lock(&data).metadata()),
            Node::Directory(dir) => Ok(dir.metadata(self.tree.dev)),
            Node::Whiteout { .. } => Ok(Metadata::new(FileType::Unknown, 0).with_nlink(1)),
        }
    }

    /// A Tmpfs matches names byte for byte, and records what a directory
    /// standing at the name is, as tmpfs's lookup hands `d_add` its inode.
    fn lookup(&self, dentry: &Dentry) -> Result<String, FilesystemError> {
        if let Ok(Node::Directory(dir)) = self.dir.get(&dentry.name()) {
            dentry.d_instantiate(Identity::of(&dir.metadata(self.tree.dev)));
        }
        Ok(dentry.name())
    }

    /// A Tmpfs handed to more than one Filesystem instance changes through
    /// each, behind the others' dentries, so a dentry whose identity the
    /// runtime learned is valid only while that directory stands at its
    /// name here, as NFS compares the handle a fresh lookup finds. One the
    /// runtime learned nothing of has nothing to compare.
    fn d_revalidate(&self, dentry: &Dentry) -> Result<bool, FilesystemError> {
        let Some(identity) = dentry.identity() else {
            return Ok(true);
        };
        Ok(match self.dir.get(&dentry.name()) {
            Ok(Node::Directory(dir)) => {
                Identity::of(&dir.metadata(self.tree.dev)) == Some(identity)
            }
            Ok(_) | Err(_) => false,
        })
    }

    /// A Tmpfs holds no links.
    fn read_link(&self, dentry: &Dentry) -> Result<String, FilesystemError> {
        self.symlink_metadata(dentry)?;
        Err(FilesystemError::InvalidInput)
    }

    fn entries(&self, _dentry: &Dentry) -> Result<Vec<DirEntry>, FilesystemError> {
        Ok(self
            .dir
            .state()
            .entries
            .iter()
            .map(|(name, node)| DirEntry::new(name.as_str(), node.file_type()))
            .collect())
    }

    fn create_dir(&self, dentry: &Dentry) -> Result<(), FilesystemError> {
        self.tree.space.inodes.take(1)?;
        let made = Node::Directory(self.new_directory());
        self.dir.state().insert(&dentry.name(), made);
        Ok(())
    }

    fn remove_dir(&self, dentry: &Dentry) -> Result<(), FilesystemError> {
        let name = &dentry.name();
        let mut state = self.dir.state();
        let removed = match state.entries.get(name) {
            Some(Node::File(_) | Node::Whiteout { .. }) => {
                return Err(FilesystemError::NotADirectory)
            }
            Some(Node::Directory(removed)) => removed.clone(),
            None => return Err(FilesystemError::NotFound),
        };
        if !removed.state().entries.is_empty() {
            return Err(FilesystemError::DirectoryNotEmpty);
        }
        state.remove(name);
        self.tree.space.inodes.give(1);
        Ok(())
    }

    fn remove_file(&self, dentry: &Dentry) -> Result<(), FilesystemError> {
        let name = &dentry.name();
        let mut state = self.dir.state();
        match state.entries.get(name) {
            Some(Node::File(_) | Node::Whiteout { .. }) => {
                state.remove(name);
                Ok(())
            }
            Some(Node::Directory(_)) => Err(FilesystemError::IsADirectory),
            None => Err(FilesystemError::NotFound),
        }
    }

    /// The move `rename2` makes with no flag, as `shmem_rename2` makes it.
    fn rename(
        &self,
        from: &Dentry,
        to_dir: &dyn Inode,
        to: &Dentry,
    ) -> Result<(), FilesystemError> {
        let target = (to_dir as &dyn Any)
            .downcast_ref::<Self>()
            .ok_or(FilesystemError::CrossesDevices)?;
        self.rename2(&from.name(), target, &to.name(), None)
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

impl File for TmpfsFile {
    fn read_at(&mut self, buf: &mut [u8], offset: u64) -> Result<usize, FilesystemError> {
        let data = &lock(&self.data).bytes;
        let start = usize::try_from(offset)
            .unwrap_or(usize::MAX)
            .min(data.len());
        let read = buf.len().min(data.len() - start);
        buf[..read].copy_from_slice(&data[start..start + read]);
        Ok(read)
    }

    fn write_at(&mut self, buf: &[u8], offset: u64) -> Result<usize, FilesystemError> {
        let start = usize::try_from(offset).map_err(|_| FilesystemError::InvalidInput)?;
        write(&mut lock(&self.data), buf, start)
    }

    /// The end is read and written under one lock, so another handle's
    /// append lands before or after this one, never over it.
    fn append(&mut self, buf: &[u8]) -> Result<(u64, usize), FilesystemError> {
        let mut data = lock(&self.data);
        let start = data.bytes.len();
        Ok((start as u64, write(&mut data, buf, start)?))
    }

    fn metadata(&self) -> Result<Metadata, FilesystemError> {
        Ok(lock(&self.data).metadata())
    }

    fn set_len(&mut self, len: u64) -> Result<(), FilesystemError> {
        let len = usize::try_from(len).map_err(|_| FilesystemError::InvalidInput)?;
        lock(&self.data).resize(len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vfs::kern_path::{kern_path, KernPath};

    /// `tmpfs` reached through the VFS.
    fn at(tmpfs: Tmpfs) -> KernPath {
        kern_path(Arc::new(tmpfs))
    }

    fn read_all(fs: &KernPath, path: &str) -> Vec<u8> {
        let mut file = fs
            .open(
                path,
                &OpenOptions {
                    read: true,
                    ..OpenOptions::default()
                },
            )
            .expect("the file must open");
        let mut buf = vec![0; 64];
        let read = file.read_at(&mut buf, 0).expect("the file must read");
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
            .open(
                "notes.txt",
                &OpenOptions {
                    write: true,
                    create: true,
                    ..OpenOptions::default()
                },
            )
            .expect("a new file must be created");
        file.write_at(b"kept", 0).expect("the write must land");
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

    fn writable(fs: &KernPath, path: &str) -> Box<dyn File> {
        fs.open(
            path,
            &OpenOptions {
                write: true,
                create: true,
                ..OpenOptions::default()
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
                writable(&tmpfs, "file").write_at(b"four", 1),
                read_all(&tmpfs, "file")
            ),
            (Err(FilesystemError::StorageFull), b"x".to_vec()),
            "a write growing a Tmpfs's files past its limit through TmpfsFile::write_at must \
             answer StorageFull and leave the file as it was"
        );
    }

    // @behavior FS-096
    #[test]
    fn a_write_of_no_bytes_leaves_a_tmpfs_file_as_it_was() {
        let tmpfs = at([("file", "")].into_iter().collect());
        assert_eq!(
            (
                writable(&tmpfs, "file").write_at(b"", 1000),
                read_all(&tmpfs, "file")
            ),
            (Ok(0), Vec::new()),
            "an empty write past the end through TmpfsFile::write_at must answer 0 and leave \
             the file empty"
        );
    }

    // @behavior FS-023
    #[test]
    fn a_tmpfs_holds_its_default_size() {
        let tmpfs = at(Tmpfs::new());
        let mut file = writable(&tmpfs, "file");
        let mib_64 = 64 * 1024 * 1024;
        assert_eq!(
            (file.set_len(mib_64), file.set_len(mib_64 + 1)),
            (Ok(()), Err(FilesystemError::StorageFull)),
            "a Tmpfs given no limit through Tmpfs::new must hold 64 MiB and refuse a byte more"
        );
    }

    // @behavior FS-024
    #[test]
    fn a_tmpfs_without_a_size_grows_past_the_default() {
        let tmpfs = at(Tmpfs::new().with_size(None));
        assert_eq!(
            writable(&tmpfs, "file").set_len(Tmpfs::DEFAULT_SIZE as u64 + 1),
            Ok(()),
            "a Tmpfs given no limit through Tmpfs::with_size(None) must grow past 64 MiB"
        );
    }

    // @behavior FS-025
    #[test]
    fn a_removed_file_returns_its_space() {
        let tmpfs = at(Tmpfs::new().with_size(Some(4)));
        writable(&tmpfs, "first")
            .write_at(b"four", 0)
            .expect("the first file must fit");
        tmpfs.remove_file("first").expect("the file must remove");
        assert_eq!(
            writable(&tmpfs, "second").write_at(b"four", 0),
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
                file.set_len(unallocatable),
                file.write_at(b"x", unallocatable),
                read_all(&tmpfs, "file")
            ),
            (
                Err(FilesystemError::StorageFull),
                Err(FilesystemError::StorageFull),
                b"x".to_vec()
            ),
            "a length no allocator can hand out through TmpfsFile::set_len and write_at must \
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
        fs.open(
            path,
            &OpenOptions {
                write: true,
                create: true,
                ..OpenOptions::default()
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
        assert_eq!(
            (
                first.append(b"bb"),
                second.append(b"ccc"),
                read_all(&tmpfs, "log")
            ),
            (Ok((1, 2)), Ok((3, 3)), b"abbccc".to_vec()),
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
        let (first, second) = (Metadata::unique_device(), Metadata::unique_device());
        let tmpfs = at(Tmpfs::new())
            .metadata()
            .expect("the root must stand")
            .dev;
        assert!(
            [first, second, tmpfs].iter().all(|dev| dev >> 63 == 1)
                && first != second
                && tmpfs != first
                && tmpfs != second,
            "devices from Metadata::unique_device and a Tmpfs's root must each set the top bit \
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
            (Ok(FileType::File), Err(FilesystemError::NotFound)),
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
                Ok(FileType::Directory)
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
            (Err(FilesystemError::InvalidInput), Ok(FileType::Directory)),
            "Tmpfs::rename of a directory into one opened beneath it must answer InvalidInput \
             and move nothing"
        );
    }

    // @behavior FS-109
    #[test]
    fn a_rename_into_a_directory_of_another_tmpfs_crosses_devices() {
        let tmpfs = at([("file", "x")].into_iter().collect());
        let other = at(Tmpfs::new());
        assert_eq!(
            (
                tmpfs.rename("file", &other, "file"),
                read_all(&tmpfs, "file")
            ),
            (Err(FilesystemError::CrossesDevices), b"x".to_vec()),
            "Tmpfs::rename into a directory of another Tmpfs must answer CrossesDevices and \
             leave the file"
        );
    }

    // The write waits past the clock's resolution, so a time left where
    // the file was made cannot pass for one the write moved.
    // @behavior FS-115
    #[test]
    fn a_tmpfs_keeps_a_files_times_as_a_tmpfs_mounted_noatime_does() {
        let tmpfs = at(Tmpfs::new());
        let options = OpenOptions {
            read: true,
            write: true,
            create: true,
            ..OpenOptions::default()
        };
        let mut file = tmpfs.open("file", &options).expect("the file must be made");
        file.write_at(b"ab", 0).expect("the file must take a write");
        let made = file.metadata().expect("the file must report");
        file.read_at(&mut [0; 1], 0).expect("the file must read");
        std::thread::sleep(std::time::Duration::from_millis(10));
        file.write_at(b"c", 0)
            .expect("the file must take a write within its length");
        let written = tmpfs.symlink_metadata("file").expect("the file must stand");
        assert_eq!(
            (
                written.accessed == made.accessed,
                written.modified > made.modified,
                written.changed == written.modified,
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
            (Some(4), Some(1)),
            "Tmpfs::metadata of a directory holding a file and two directories must count four \
             names, and of the file one"
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
                .len
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
}
