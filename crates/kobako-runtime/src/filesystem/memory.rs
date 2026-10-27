//! `Memory`: a Filesystem held in memory, living as long as the Host
//! App's object does.

use std::any::Any;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::SystemTime;

use super::path::{ancestors, components, name, normalize_seed, parent};
use super::{DirEntry, File, FileType, Filesystem, FilesystemError, Metadata, OpenOptions, Rename};
use crate::vfs::Dentry;

/// Files and directories held in memory. Every Invocation it is handed to
/// shares it, so what one writes the next reads. What its files hold
/// together, and how many entries it holds, are bounded as a tmpfs mount
/// bounds its size and its inodes, so a guest cannot grow it past what
/// the Host App allowed. A directory opened beneath it is a Memory too,
/// following that directory wherever it moves, as a tmpfs descriptor
/// follows its inode.
pub struct Memory {
    tree: Arc<Tree>,
    dir: Arc<Dir>,
}

/// What every directory of one Memory shares.
struct Tree {
    /// Held across each call, so a change is never seen half made and a
    /// move finds the directories above its target as they stand, as
    /// Linux serializes moves between directories under one lock.
    lock: Mutex<()>,
    space: Arc<Space>,
    inodes_follow_limit: AtomicBool,
    /// The device number this Memory reports, its own as each tmpfs mount
    /// has its own, so it shares an identity with no other Filesystem.
    dev: u64,
    /// The inode number the next directory takes, so each one made has an
    /// identity no other directory of this Memory had.
    next_ino: AtomicU64,
}

#[derive(Clone)]
enum Node {
    File(Arc<Mutex<Content>>),
    Directory(Arc<Dir>),
}

impl Node {
    fn file_type(&self) -> FileType {
        match self {
            Self::File(_) => FileType::File,
            Self::Directory(_) => FileType::Directory,
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
    parent: Weak<Dir>,
    /// Whether the directory was removed or replaced, so nothing is made
    /// in it any more, as Linux marks such a directory dead.
    removed: bool,
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

    fn report(self, metadata: Metadata) -> Metadata {
        metadata.with_times(Some(self.made), Some(self.changed), Some(self.changed))
    }
}

impl Dir {
    fn new(ino: u64, parent: Weak<Dir>) -> Arc<Self> {
        Arc::new(Self {
            ino,
            state: Mutex::new(State {
                entries: BTreeMap::new(),
                parent,
                removed: false,
                times: Times::now(),
            }),
        })
    }

    /// What the directory reports: two names and one more for each
    /// directory it holds, as a tmpfs counts `.` and each `..` beneath it.
    fn metadata(&self, dev: u64) -> Metadata {
        let state = self.state();
        let subdirectories = state
            .entries
            .values()
            .filter(|node| matches!(node, Node::Directory(_)))
            .count() as u64;
        state.times.report(
            Metadata::new(FileType::Directory, 0)
                .with_identity(dev, self.ino)
                .with_nlink(2 + subdirectories),
        )
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// What `path` names beneath this directory, or why it names nothing:
    /// a file standing where a directory above it should be, or simply no
    /// entry.
    fn find(self: &Arc<Self>, path: &str) -> Result<Node, FilesystemError> {
        let mut found = Node::Directory(self.clone());
        for name in components(path) {
            found = match found {
                Node::Directory(dir) => dir
                    .state()
                    .entries
                    .get(name)
                    .cloned()
                    .ok_or(FilesystemError::NotFound)?,
                Node::File(_) => return Err(FilesystemError::NotADirectory),
            };
        }
        Ok(found)
    }

    /// The directory `path` names beneath this one.
    fn directory(self: &Arc<Self>, path: &str) -> Result<Arc<Self>, FilesystemError> {
        match self.find(path)? {
            Node::Directory(dir) => Ok(dir),
            Node::File(_) => Err(FilesystemError::NotADirectory),
        }
    }

    /// The directory a new entry at `path` goes into, which must stand and
    /// not have been removed, and the entry's name there.
    fn parent_of<'a>(
        self: &Arc<Self>,
        path: &'a str,
    ) -> Result<(Arc<Self>, &'a str), FilesystemError> {
        let dir = self.directory(parent(path))?;
        if dir.state().removed {
            return Err(FilesystemError::NotFound);
        }
        Ok((dir, name(path)))
    }

    /// Whether this directory is `dir` or lies beneath it.
    fn within(self: &Arc<Self>, dir: &Arc<Self>) -> bool {
        let mut at = Some(self.clone());
        while let Some(current) = at {
            if Arc::ptr_eq(&current, dir) {
                return true;
            }
            at = current.state().parent.upgrade();
        }
        false
    }
}

/// What a Memory holds: the bytes of its files, and its entries, each
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

/// One file's bytes, counted against its Memory's space, with the file's
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

    /// What the file reports, named once, as a Memory holds no hard links.
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

impl Memory {
    /// The most bytes a Memory's files hold unless given a limit of their
    /// own: the size Docker gives a `--tmpfs` mount.
    pub const DEFAULT_LIMIT: usize = 64 * 1024 * 1024;

    pub fn new() -> Self {
        let space = Space {
            bytes: Budget::new(Some(Self::DEFAULT_LIMIT)),
            inodes: Budget::new(inodes_for(Some(Self::DEFAULT_LIMIT))),
        };
        space.inodes.add(1);
        Self {
            tree: Arc::new(Tree {
                lock: Mutex::new(()),
                space: Arc::new(space),
                inodes_follow_limit: AtomicBool::new(true),
                dev: Metadata::unique_device(),
                next_ino: AtomicU64::new(2),
            }),
            dir: Dir::new(1, Weak::new()),
        }
    }

    /// A directory in `parent` with an inode number no directory of this
    /// Memory had.
    fn new_directory(&self, parent: &Arc<Dir>) -> Arc<Dir> {
        Dir::new(
            self.tree.next_ino.fetch_add(1, Ordering::Relaxed),
            Arc::downgrade(parent),
        )
    }

    /// This Memory with its files holding at most `limit` bytes together,
    /// or without bound for `None`. Unless given inodes of their own, its
    /// entries follow the limit: one for each 4 KiB it spans, and the
    /// root. What it was seeded with counts, but is never refused.
    pub fn with_limit(self, limit: Option<usize>) -> Self {
        self.tree.space.bytes.set_limit(limit);
        if self.tree.inodes_follow_limit.load(Ordering::Relaxed) {
            self.tree.space.inodes.set_limit(inodes_for(limit));
        }
        self
    }

    /// This Memory holding at most `inodes` entries, the root among them,
    /// or without bound for `None`, as a tmpfs mount's `nr_inodes` does.
    pub fn with_inodes(self, inodes: Option<usize>) -> Self {
        self.tree.space.inodes.set_limit(inodes);
        self.tree
            .inodes_follow_limit
            .store(false, Ordering::Relaxed);
        self
    }

    fn hold(&self) -> MutexGuard<'_, ()> {
        self.tree
            .lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// The directory `dir` of this Memory, as a Memory of its own.
    fn beneath(&self, dir: Arc<Dir>) -> Self {
        Self {
            tree: self.tree.clone(),
            dir,
        }
    }
}

impl Default for Memory {
    fn default() -> Self {
        Self::new()
    }
}

/// Why a list of files cannot seed a Memory as given.
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
            Self::Root => write!(f, "a Memory seeds no file at its root"),
            Self::Twice(path) => write!(f, "a Memory seeds {path} twice"),
            Self::FileAndDirectory(path) => {
                write!(f, "a Memory seeds {path} as both a file and a directory")
            }
            Self::DotDot(path) => write!(f, "a Memory seeds no path through `..`, got {path}"),
        }
    }
}

impl std::error::Error for SeedError {}

impl Memory {
    /// A Memory seeded with files, each `(path, contents)`, its directories
    /// created along the way, or why no Memory holds the list as given.
    pub fn try_from_iter<P, C, I>(files: I) -> Result<Self, SeedError>
    where
        P: AsRef<str>,
        C: Into<Vec<u8>>,
        I: IntoIterator<Item = (P, C)>,
    {
        let memory = Self::new();
        let space = &memory.tree.space;
        for (path, contents) in files {
            let path = normalize_seed(path.as_ref());
            if path.is_empty() {
                return Err(SeedError::Root);
            }
            if path.split('/').any(|name| name == "..") {
                return Err(SeedError::DotDot(path));
            }
            let mut dir = memory.dir.clone();
            for above in ancestors(&path) {
                let next = dir
                    .state()
                    .entries
                    .entry(name(above).to_string())
                    .or_insert_with(|| {
                        space.inodes.add(1);
                        Node::Directory(memory.new_directory(&dir))
                    })
                    .clone();
                dir = match next {
                    Node::Directory(inner) => inner,
                    Node::File(_) => return Err(SeedError::FileAndDirectory(above.to_string())),
                };
            }
            let mut state = dir.state();
            match state.entries.get(name(&path)) {
                Some(Node::Directory(_)) => return Err(SeedError::FileAndDirectory(path)),
                Some(Node::File(_)) => return Err(SeedError::Twice(path)),
                None => {}
            }
            state.entries.insert(
                name(&path).to_string(),
                Node::File(Content::seed(contents.into(), space)),
            );
        }
        Ok(memory)
    }
}

/// A Memory seeded with files, each `(path, contents)`, its directories
/// created along the way.
///
/// # Panics
///
/// When `Memory::try_from_iter` refuses the list: a list written into the
/// Host App's own code that no Memory holds is a defect in that code.
impl<P: AsRef<str>, C: Into<Vec<u8>>> FromIterator<(P, C)> for Memory {
    fn from_iter<I: IntoIterator<Item = (P, C)>>(files: I) -> Self {
        Self::try_from_iter(files).unwrap_or_else(|error| panic!("{error}"))
    }
}

impl Filesystem for Memory {
    fn open(
        &self,
        dentry: &Dentry,
        options: &OpenOptions,
    ) -> Result<Box<dyn File>, FilesystemError> {
        let path = &dentry.name();
        let _held = self.hold();
        let data = match self.dir.find(path) {
            Ok(Node::Directory(_)) => return Err(FilesystemError::IsADirectory),
            Ok(Node::File(_)) if options.create_new => return Err(FilesystemError::AlreadyExists),
            Ok(Node::File(data)) => {
                if options.truncate {
                    lock(&data).resize(0)?;
                }
                data
            }
            Err(FilesystemError::NotFound) if options.create || options.create_new => {
                let (dir, name) = self.dir.parent_of(path)?;
                let data = Content::create(&self.tree.space)?;
                dir.state().insert(name, Node::File(data.clone()));
                data
            }
            Err(error) => return Err(error),
        };
        Ok(Box::new(MemoryFile { data }))
    }

    /// A Memory holds no links to refuse.
    fn open_dir(&self, dentry: &Dentry) -> Result<Arc<dyn Filesystem>, FilesystemError> {
        let _held = self.hold();
        Ok(Arc::new(self.beneath(self.dir.directory(&dentry.name())?)))
    }

    fn metadata(&self) -> Result<Metadata, FilesystemError> {
        let _held = self.hold();
        Ok(self.dir.metadata(self.tree.dev))
    }

    /// A Memory holds no links.
    fn symlink_metadata(&self, dentry: &Dentry) -> Result<Metadata, FilesystemError> {
        let _held = self.hold();
        match self.dir.find(&dentry.name())? {
            Node::File(data) => Ok(lock(&data).metadata()),
            Node::Directory(dir) => Ok(dir.metadata(self.tree.dev)),
        }
    }

    /// A Memory matches names byte for byte.
    fn stored_name(&self, dentry: &Dentry) -> Result<String, FilesystemError> {
        Ok(dentry.name())
    }

    /// A Memory holds no links.
    fn read_link(&self, dentry: &Dentry) -> Result<String, FilesystemError> {
        self.symlink_metadata(dentry)?;
        Err(FilesystemError::InvalidInput)
    }

    fn entries(&self) -> Result<Vec<DirEntry>, FilesystemError> {
        let _held = self.hold();
        Ok(self
            .dir
            .state()
            .entries
            .iter()
            .map(|(name, node)| DirEntry::new(name.as_str(), node.file_type()))
            .collect())
    }

    fn create_dir(&self, dentry: &Dentry) -> Result<(), FilesystemError> {
        let path = &dentry.name();
        let _held = self.hold();
        if self.dir.find(path).is_ok() {
            return Err(FilesystemError::AlreadyExists);
        }
        let (dir, name) = self.dir.parent_of(path)?;
        self.tree.space.inodes.take(1)?;
        let made = Node::Directory(self.new_directory(&dir));
        dir.state().insert(name, made);
        Ok(())
    }

    /// A directory removed while opened is marked so, and nothing is made
    /// in it any more.
    fn remove_dir(&self, dentry: &Dentry) -> Result<(), FilesystemError> {
        let path = &dentry.name();
        let _held = self.hold();
        let (dir, name) = self.dir.parent_of(path)?;
        let mut state = dir.state();
        let removed = match state.entries.get(name) {
            Some(Node::File(_)) => return Err(FilesystemError::NotADirectory),
            Some(Node::Directory(removed)) => removed.clone(),
            None => return Err(FilesystemError::NotFound),
        };
        let mut removed_state = removed.state();
        if !removed_state.entries.is_empty() {
            return Err(FilesystemError::DirectoryNotEmpty);
        }
        removed_state.removed = true;
        state.remove(name);
        self.tree.space.inodes.give(1);
        Ok(())
    }

    fn remove_file(&self, dentry: &Dentry) -> Result<(), FilesystemError> {
        let path = &dentry.name();
        let _held = self.hold();
        let (dir, name) = self.dir.parent_of(path)?;
        let mut state = dir.state();
        match state.entries.get(name) {
            Some(Node::File(_)) => {
                state.remove(name);
                Ok(())
            }
            Some(Node::Directory(_)) => Err(FilesystemError::IsADirectory),
            None => Err(FilesystemError::NotFound),
        }
    }

    /// The entry moves under its new name with all it holds, so no name
    /// beneath it changes. A directory never moves into itself or beneath
    /// itself, which would cut it off from the root for good, as Linux
    /// refuses it once it finds the source above the target.
    fn rename(
        &self,
        from: &Dentry,
        to_dir: &dyn Filesystem,
        to: &Dentry,
        rename: Rename,
    ) -> Result<(), FilesystemError> {
        let target = (to_dir as &dyn Any)
            .downcast_ref::<Self>()
            .filter(|target| Arc::ptr_eq(&target.tree, &self.tree))
            .ok_or(FilesystemError::CrossesDevices)?;
        let (from, to) = (&from.name(), &to.name());
        let _held = self.hold();
        let moving = self.dir.find(from)?;
        let (from_dir, from_name) = self.dir.parent_of(from)?;
        let (to_dir, to_name) = target.dir.parent_of(to)?;
        if Arc::ptr_eq(&from_dir, &to_dir) && from_name == to_name {
            return Ok(());
        }
        rename.check(moving.file_type())?;
        if let Node::Directory(moving) = &moving {
            if to_dir.within(moving) {
                return Err(FilesystemError::InvalidInput);
            }
        }
        let replaced = to_dir.state().entries.get(to_name).cloned();
        match (&moving, &replaced) {
            (Node::File(_), Some(Node::Directory(_))) => return Err(FilesystemError::IsADirectory),
            (Node::Directory(_), Some(Node::File(_))) => {
                return Err(FilesystemError::NotADirectory)
            }
            (Node::Directory(_), Some(Node::Directory(dir))) if !dir.state().entries.is_empty() => {
                return Err(FilesystemError::DirectoryNotEmpty)
            }
            _ => {}
        }
        from_dir.state().remove(from_name);
        if let Node::Directory(moved) = &moving {
            moved.state().parent = Arc::downgrade(&to_dir);
        }
        to_dir.state().insert(to_name, moving);
        if let Some(Node::Directory(replaced)) = replaced {
            replaced.state().removed = true;
            self.tree.space.inodes.give(1);
        }
        Ok(())
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

/// An open file of a Memory, sharing its bytes with every other handle on
/// the same path.
struct MemoryFile {
    data: Arc<Mutex<Content>>,
}

impl File for MemoryFile {
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
    use crate::vfs::through::{through, Through};

    /// `memory` reached through the VFS.
    fn at(memory: Memory) -> Through {
        through(Arc::new(memory))
    }

    fn read_all(fs: &Through, path: &str) -> Vec<u8> {
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
    fn a_memory_serves_the_files_it_was_seeded_with() {
        let memory = at([("/config/app.yml", "name: kobako")].into_iter().collect());
        assert_eq!(
            read_all(&memory, "config/app.yml"),
            b"name: kobako",
            "a file seeded into Memory through FromIterator must read back its content"
        );
    }

    // @behavior FS-067
    #[test]
    fn a_memory_tried_with_a_list_it_cannot_hold_answers_why() {
        let refused = [
            Memory::try_from_iter([("/", "x")]),
            Memory::try_from_iter([("a", "x"), ("a/b", "y")]),
            Memory::try_from_iter([("a", "x"), ("/a", "y")]),
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
             Memory::try_from_iter must answer the SeedError naming it"
        );
    }

    // A runtime never hands a Filesystem `..`, so a file seeded through one
    // would be held where no call reaches it.
    // @behavior FS-087
    #[test]
    fn a_memory_tried_with_a_path_through_dot_dot_answers_why() {
        assert_eq!(
            Memory::try_from_iter([("a/../b", "x")]).map(drop),
            Err(SeedError::DotDot("a/../b".to_string())),
            "a list naming a path through `..` through Memory::try_from_iter must answer \
             SeedError::DotDot naming it"
        );
    }

    // @behavior FS-062
    #[test]
    #[should_panic(expected = "a Memory seeds no file at its root")]
    fn a_memory_seeded_at_its_root_panics() {
        let _: Memory = [("/", "x")].into_iter().collect();
    }

    // @behavior FS-062
    #[test]
    #[should_panic(expected = "a Memory seeds a as both a file and a directory")]
    fn a_memory_seeded_with_a_file_beneath_a_file_panics() {
        let _: Memory = [("a", "x"), ("a/b", "y")].into_iter().collect();
    }

    // @behavior FS-062
    #[test]
    #[should_panic(expected = "a Memory seeds a as both a file and a directory")]
    fn a_memory_seeded_with_a_file_over_a_directory_panics() {
        let _: Memory = [("a/b", "y"), ("a", "x")].into_iter().collect();
    }

    // @behavior FS-062
    #[test]
    #[should_panic(expected = "a Memory seeds a twice")]
    fn a_memory_seeded_with_one_path_twice_panics() {
        let _: Memory = [("a", "x"), ("a", "y")].into_iter().collect();
    }

    // @behavior FS-002
    #[test]
    fn a_memory_keeps_what_was_written() {
        let memory = at(Memory::new());
        let mut file = memory
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
            read_all(&memory, "notes.txt"),
            b"kept",
            "a file written through Memory::open must read back from the same Memory"
        );
    }

    #[test]
    fn a_directory_with_entries_is_not_removed() {
        let memory = at([("dir/file", "x")].into_iter().collect());
        assert_eq!(
            memory.remove_dir("dir"),
            Err(FilesystemError::DirectoryNotEmpty)
        );
        assert_eq!(memory.remove_file("dir/file"), Ok(()));
        assert_eq!(memory.remove_dir("dir"), Ok(()));
        assert_eq!(
            memory.symlink_metadata("dir"),
            Err(FilesystemError::NotFound)
        );
    }

    #[test]
    fn a_renamed_directory_carries_its_entries() {
        let memory = at([("old/inner/file", "x")].into_iter().collect());
        memory
            .rename("old", &memory, "new", Rename::Any)
            .expect("the rename must succeed");
        assert_eq!(read_all(&memory, "new/inner/file"), b"x");
        assert_eq!(
            memory.symlink_metadata("old"),
            Err(FilesystemError::NotFound)
        );
    }

    fn writable(fs: &Through, path: &str) -> Box<dyn File> {
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
    fn a_write_past_a_memorys_limit_is_refused() {
        let memory: Memory = [("file", "x")].into_iter().collect();
        let memory = at(memory.with_limit(Some(4)));
        assert_eq!(
            (
                writable(&memory, "file").write_at(b"four", 1),
                read_all(&memory, "file")
            ),
            (Err(FilesystemError::StorageFull), b"x".to_vec()),
            "a write growing a Memory's files past its limit through MemoryFile::write_at must \
             answer StorageFull and leave the file as it was"
        );
    }

    // @behavior FS-096
    #[test]
    fn a_write_of_no_bytes_leaves_a_memory_file_as_it_was() {
        let memory = at([("file", "")].into_iter().collect());
        assert_eq!(
            (
                writable(&memory, "file").write_at(b"", 1000),
                read_all(&memory, "file")
            ),
            (Ok(0), Vec::new()),
            "an empty write past the end through MemoryFile::write_at must answer 0 and leave \
             the file empty"
        );
    }

    // @behavior FS-023
    #[test]
    fn a_memory_holds_its_default_limit() {
        let memory = at(Memory::new());
        let mut file = writable(&memory, "file");
        let mib_64 = 64 * 1024 * 1024;
        assert_eq!(
            (file.set_len(mib_64), file.set_len(mib_64 + 1)),
            (Ok(()), Err(FilesystemError::StorageFull)),
            "a Memory given no limit through Memory::new must hold 64 MiB and refuse a byte more"
        );
    }

    // @behavior FS-024
    #[test]
    fn a_memory_without_a_limit_grows_past_the_default() {
        let memory = at(Memory::new().with_limit(None));
        assert_eq!(
            writable(&memory, "file").set_len(Memory::DEFAULT_LIMIT as u64 + 1),
            Ok(()),
            "a Memory given no limit through Memory::with_limit(None) must grow past 64 MiB"
        );
    }

    // @behavior FS-025
    #[test]
    fn a_removed_file_returns_its_space() {
        let memory = at(Memory::new().with_limit(Some(4)));
        writable(&memory, "first")
            .write_at(b"four", 0)
            .expect("the first file must fit");
        memory.remove_file("first").expect("the file must remove");
        assert_eq!(
            writable(&memory, "second").write_at(b"four", 0),
            Ok(4),
            "the space a file removed through Memory::remove_file held must be free for the next"
        );
    }

    // A length past `isize::MAX` is one no allocator can hand out, so the
    // refusal does not depend on how much the host has free.
    // @behavior FS-026
    #[test]
    fn a_file_grown_past_what_can_be_allocated_is_refused() {
        let memory: Memory = [("file", "x")].into_iter().collect();
        let memory = at(memory.with_limit(None));
        let mut file = writable(&memory, "file");
        let unallocatable = isize::MAX as u64 + 1;
        assert_eq!(
            (
                file.set_len(unallocatable),
                file.write_at(b"x", unallocatable),
                read_all(&memory, "file")
            ),
            (
                Err(FilesystemError::StorageFull),
                Err(FilesystemError::StorageFull),
                b"x".to_vec()
            ),
            "a length no allocator can hand out through MemoryFile::set_len and write_at must \
             answer StorageFull rather than abort"
        );
    }

    #[test]
    fn a_path_beneath_a_file_is_not_a_directory() {
        let memory = at([("file", "x")].into_iter().collect());
        assert_eq!(
            memory.symlink_metadata("file/inner"),
            Err(FilesystemError::NotADirectory)
        );
    }

    fn create(fs: &Through, path: &str) -> Result<(), FilesystemError> {
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
    fn a_memory_holds_at_most_its_inodes() {
        let memory = at(Memory::new().with_inodes(Some(3)));
        assert_eq!(
            (
                create(&memory, "file"),
                memory.create_dir("dir"),
                create(&memory, "past"),
                memory.create_dir("past_dir"),
            ),
            (
                Ok(()),
                Ok(()),
                Err(FilesystemError::StorageFull),
                Err(FilesystemError::StorageFull)
            ),
            "a Memory given inodes through Memory::with_inodes must refuse a file or directory \
             past them, the root counted, as out of space"
        );
    }

    // @behavior FS-031
    #[test]
    fn a_memorys_inodes_follow_its_limit() {
        let memory = at(Memory::new().with_limit(Some(2 * 4096)));
        assert_eq!(
            (
                create(&memory, "a"),
                create(&memory, "b"),
                create(&memory, "c")
            ),
            (Ok(()), Ok(()), Err(FilesystemError::StorageFull)),
            "a Memory given only a limit through Memory::with_limit must hold one entry for each \
             4 KiB of it and the root"
        );
    }

    // The second handle opened before the first appended, so only an end
    // read as the append lands puts its bytes after the first's.
    // @behavior FS-036
    #[test]
    fn a_memory_appends_at_the_end_as_it_stands() {
        let memory = at([("log", "a")].into_iter().collect());
        let mut first = writable(&memory, "log");
        let mut second = writable(&memory, "log");
        assert_eq!(
            (
                first.append(b"bb"),
                second.append(b"ccc"),
                read_all(&memory, "log")
            ),
            (Ok((1, 2)), Ok((3, 3)), b"abbccc".to_vec()),
            "MemoryFile::append through two handles on one file must land each at the end as it \
             stands, answering where it landed"
        );
    }

    // @behavior FS-035
    #[test]
    fn a_memory_moves_only_what_a_rename_allows() {
        let memory = at([("dir/inner", "x"), ("file", "y")].into_iter().collect());
        assert_eq!(
            (
                memory.rename("dir", &memory, "moved", Rename::File),
                memory.rename("file", &memory, "moved", Rename::Directory),
                memory.symlink_metadata("dir/inner").map(|m| m.file_type),
                memory.symlink_metadata("file").map(|m| m.file_type),
            ),
            (
                Err(FilesystemError::CrossesDevices),
                Err(FilesystemError::NotADirectory),
                Ok(FileType::File),
                Ok(FileType::File)
            ),
            "Memory::rename must refuse a directory a file-only rename names as crossing \
             devices, and a file a directory-only rename names as no directory, moving neither"
        );
    }

    // A directory replaced by a rename gives its inode back as a removed
    // one does, so both leave room for the next.
    // @behavior FS-034
    #[test]
    fn a_removed_directory_gives_its_inode_back() {
        let memory = at(Memory::new().with_inodes(Some(3)));
        memory
            .create_dir("a")
            .expect("the first directory must fit");
        memory
            .create_dir("b")
            .expect("the second directory must fit");
        memory.remove_dir("a").expect("the directory must remove");
        let after_removal = memory.create_dir("c");
        memory
            .rename("c", &memory, "b", Rename::Any)
            .expect("a directory must replace an empty one");
        assert_eq!(
            (after_removal, memory.create_dir("d")),
            (Ok(()), Ok(())),
            "a directory removed through Memory::remove_dir, or replaced through Memory::rename, \
             must give its inode back"
        );
    }

    // The removed file's handle is still open, so its inode is given back
    // only when the handle goes, as Linux keeps an unlinked file's inode.
    // @behavior FS-032
    #[test]
    fn a_removed_file_gives_its_inode_back_once_closed() {
        let memory = at(Memory::new().with_inodes(Some(2)));
        let handle = writable(&memory, "first");
        memory.remove_file("first").expect("the file must remove");
        let while_open = create(&memory, "second");
        drop(handle);
        assert_eq!(
            (while_open, create(&memory, "second")),
            (Err(FilesystemError::StorageFull), Ok(())),
            "the inode of a file removed through Memory::remove_file must stay taken while a \
             handle holds it, and be free once it closes"
        );
    }

    // A host's `st_dev` never sets the top bit, so a device carrying it
    // cannot name a host's device, nor one another Filesystem took.
    // @behavior FS-085
    #[test]
    fn a_device_of_its_own_stands_apart_from_every_other_device() {
        let (first, second) = (Metadata::unique_device(), Metadata::unique_device());
        let memory = at(Memory::new())
            .metadata()
            .expect("the root must stand")
            .dev;
        assert!(
            [first, second, memory].iter().all(|dev| dev >> 63 == 1)
                && first != second
                && memory != first
                && memory != second,
            "devices from Metadata::unique_device and a Memory's root must each set the top bit \
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
                    Node::File(_) => Vec::new(),
                };
                std::iter::once(name.clone()).chain(beneath)
            })
            .collect()
    }

    // Each round nests the file one level deeper, so a Memory holding
    // whole paths would hold names growing with the depth; held under its
    // own name, every entry stays the one letter it was given.
    // @behavior FS-094
    #[test]
    fn a_memory_holds_each_entry_under_its_own_name() {
        let held: Arc<Memory> = Arc::new([("d/f", "x")].into_iter().collect());
        let memory = through(held.clone());
        for _ in 0..64 {
            memory
                .create_dir("n")
                .expect("the new directory must be made");
            memory
                .rename("d", &memory, "n/d", Rename::Any)
                .expect("the directory must move into the new one");
            memory
                .rename("n", &memory, "d", Rename::Any)
                .expect("the new directory must take the old name");
        }
        let held = names(&held.dir);
        assert_eq!(
            (held, read_all(&memory, &format!("{}f", "d/".repeat(65)))),
            (
                std::iter::repeat_n("d".to_string(), 65)
                    .chain(["f".to_string()])
                    .collect::<Vec<_>>(),
                b"x".to_vec()
            ),
            "a directory moved deeper 64 times through Memory::rename must leave every entry \
             holding only its own name, and the file reading where it now lies"
        );
    }

    // @behavior FS-105
    #[test]
    fn a_directory_opened_from_a_memory_follows_it_when_it_moves() {
        let memory = at([("dir/.keep", "")].into_iter().collect());
        let dir = memory.open_dir("dir").expect("the directory must open");
        memory
            .rename("dir", &memory, "moved", Rename::Any)
            .expect("the directory must move");
        create(&dir, "made").expect("the file must be made");
        assert_eq!(
            (
                memory.symlink_metadata("moved/made").map(|m| m.file_type),
                memory.symlink_metadata("dir/made").map(|m| m.file_type)
            ),
            (Ok(FileType::File), Err(FilesystemError::NotFound)),
            "a file made through a directory Memory::open_dir opened before Memory::rename moved \
             it must stand beneath the directory's new path"
        );
    }

    // @behavior FS-106
    #[test]
    fn a_memory_directory_removed_while_opened_makes_nothing_more() {
        let memory = at(Memory::new());
        memory
            .create_dir("dir")
            .expect("the directory must be made");
        let dir = memory.open_dir("dir").expect("the directory must open");
        memory.remove_dir("dir").expect("the directory must remove");
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
            "a directory Memory::open_dir opened, once Memory::remove_dir removed it, must make \
             nothing, list nothing, and still stat as a directory"
        );
    }

    // @behavior FS-107
    #[test]
    fn a_memory_directory_never_moves_beneath_itself() {
        let memory = at(Memory::new());
        memory.create_dir("a").expect("the directory must be made");
        memory
            .create_dir("a/b")
            .expect("the inner directory must be made");
        let inner = memory.open_dir("a/b").expect("the directory must open");
        assert_eq!(
            (
                memory.rename("a", &inner, "a", Rename::Any),
                memory.symlink_metadata("a/b").map(|m| m.file_type)
            ),
            (Err(FilesystemError::InvalidInput), Ok(FileType::Directory)),
            "Memory::rename of a directory into one opened beneath it must answer InvalidInput \
             and move nothing"
        );
    }

    // @behavior FS-109
    #[test]
    fn a_rename_into_a_directory_of_another_memory_crosses_devices() {
        let memory = at([("file", "x")].into_iter().collect());
        let other = at(Memory::new());
        assert_eq!(
            (
                memory.rename("file", &other, "file", Rename::Any),
                read_all(&memory, "file")
            ),
            (Err(FilesystemError::CrossesDevices), b"x".to_vec()),
            "Memory::rename into a directory of another Memory must answer CrossesDevices and \
             leave the file"
        );
    }

    // The write waits past the clock's resolution, so a time left where
    // the file was made cannot pass for one the write moved.
    // @behavior FS-115
    #[test]
    fn a_memory_keeps_a_files_times_as_a_tmpfs_mounted_noatime_does() {
        let memory = at(Memory::new());
        let options = OpenOptions {
            read: true,
            write: true,
            create: true,
            ..OpenOptions::default()
        };
        let mut file = memory
            .open("file", &options)
            .expect("the file must be made");
        file.write_at(b"ab", 0).expect("the file must take a write");
        let made = file.metadata().expect("the file must report");
        file.read_at(&mut [0; 1], 0).expect("the file must read");
        std::thread::sleep(std::time::Duration::from_millis(10));
        file.write_at(b"c", 0)
            .expect("the file must take a write within its length");
        let written = memory
            .symlink_metadata("file")
            .expect("the file must stand");
        assert_eq!(
            (
                written.accessed == made.accessed,
                written.modified > made.modified,
                written.changed == written.modified,
            ),
            (true, true, true),
            "Memory::metadata of a file read and then written must keep its access time from \
             when it was made, and move its modification and change times to the write"
        );
    }

    // @behavior FS-116
    #[test]
    fn a_memory_counts_a_directorys_names_as_a_tmpfs_does() {
        let memory = at([("dir/file", "x")].into_iter().collect());
        memory
            .create_dir("dir/a")
            .expect("a directory must be made");
        memory
            .create_dir("dir/b")
            .expect("a directory must be made");
        let nlink = |path| {
            memory
                .symlink_metadata(path)
                .expect("the entry must stand")
                .nlink
        };
        assert_eq!(
            (nlink("dir"), nlink("dir/file")),
            (Some(4), Some(1)),
            "Memory::metadata of a directory holding a file and two directories must count four \
             names, and of the file one"
        );
    }

    // @behavior FS-070
    #[test]
    fn a_memory_gives_each_directory_it_makes_an_identity_of_its_own() {
        let memory = at(Memory::new());
        memory
            .create_dir("dir")
            .expect("the directory must be made");
        let identity = |path| {
            let metadata = memory
                .symlink_metadata(path)
                .expect("the directory must stand");
            (metadata.dev, metadata.ino)
        };
        let before = identity("dir");
        memory
            .rename("dir", &memory, "moved", Rename::Any)
            .expect("the directory must move");
        memory
            .create_dir("dir")
            .expect("another directory must be made");
        assert_eq!(
            (identity("moved") == before, identity("dir") == before),
            (true, false),
            "a directory moved through Memory::rename must keep its identity, and one made at its \
             old path through Memory::create_dir must have another"
        );
    }
}
