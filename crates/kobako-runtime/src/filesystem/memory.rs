//! `Memory`: a Filesystem held in memory, living as long as the Host
//! App's object does.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use super::path::{beneath_itself, is_beneath, normalize_seed, parent};
use super::{DirEntry, File, FileType, Filesystem, FilesystemError, Metadata, OpenOptions, Rename};

/// Files and directories held in memory. Every Invocation it is handed to
/// shares it, so what one writes the next reads. What its files hold
/// together, and how many entries it holds, are bounded as a tmpfs mount
/// bounds its size and its inodes, so a guest cannot grow it past what
/// the Host App allowed.
pub struct Memory {
    nodes: Mutex<BTreeMap<String, Node>>,
    space: Arc<Space>,
    inodes_follow_limit: bool,
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
    /// A directory and its inode number.
    Directory(u64),
}

impl Node {
    fn file_type(&self) -> FileType {
        match self {
            Self::File(_) => FileType::File,
            Self::Directory(_) => FileType::Directory,
        }
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

    fn limit(&self) -> Option<usize> {
        Some(self.limit.load(Ordering::Relaxed)).filter(|&limit| limit != usize::MAX)
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
        }))
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
            nodes: Mutex::new(BTreeMap::from([(String::new(), Node::Directory(1))])),
            space: Arc::new(space),
            inodes_follow_limit: true,
            dev: Metadata::unique_device(),
            next_ino: AtomicU64::new(2),
        }
    }

    /// A directory with an inode number no directory of this Memory had.
    fn new_directory(&self) -> Node {
        Node::Directory(self.next_ino.fetch_add(1, Ordering::Relaxed))
    }

    /// This Memory with its files holding at most `limit` bytes together,
    /// or without bound for `None`. Unless given inodes of their own, its
    /// entries follow the limit: one for each 4 KiB it spans, and the
    /// root. What it was seeded with counts, but is never refused.
    pub fn with_limit(self, limit: Option<usize>) -> Self {
        self.space.bytes.set_limit(limit);
        if self.inodes_follow_limit {
            self.space.inodes.set_limit(inodes_for(limit));
        }
        self
    }

    /// This Memory holding at most `inodes` entries, the root among them,
    /// or without bound for `None`, as a tmpfs mount's `nr_inodes` does.
    pub fn with_inodes(mut self, inodes: Option<usize>) -> Self {
        self.space.inodes.set_limit(inodes);
        self.inodes_follow_limit = false;
        self
    }

    /// An empty Memory bounded as this one is.
    pub(crate) fn to_empty(&self) -> Self {
        let memory = Self::new().with_limit(self.space.bytes.limit());
        if self.inodes_follow_limit {
            memory
        } else {
            memory.with_inodes(self.space.inodes.limit())
        }
    }

    fn nodes(&self) -> MutexGuard<'_, BTreeMap<String, Node>> {
        self.nodes
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
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
        {
            let mut nodes = memory.nodes();
            for (path, contents) in files {
                let path = normalize_seed(path.as_ref());
                if path.is_empty() {
                    return Err(SeedError::Root);
                }
                if path.split('/').any(|name| name == "..") {
                    return Err(SeedError::DotDot(path));
                }
                for (end, _) in path.match_indices('/') {
                    let dir = &path[..end];
                    match nodes.get(dir) {
                        Some(Node::Directory(_)) => {}
                        Some(_) => return Err(SeedError::FileAndDirectory(dir.to_string())),
                        None => {
                            nodes.insert(dir.to_string(), memory.new_directory());
                            memory.space.inodes.add(1);
                        }
                    }
                }
                match nodes.get(&path) {
                    Some(Node::Directory(_)) => return Err(SeedError::FileAndDirectory(path)),
                    Some(_) => return Err(SeedError::Twice(path)),
                    None => {}
                }
                nodes.insert(
                    path,
                    Node::File(Content::seed(contents.into(), &memory.space)),
                );
            }
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

/// Why `path` names nothing: a file standing where a directory above it
/// should be, or simply no entry.
fn missing(nodes: &BTreeMap<String, Node>, path: &str) -> FilesystemError {
    let mut dir = parent(path);
    loop {
        match nodes.get(dir) {
            Some(Node::File(_)) => return FilesystemError::NotADirectory,
            Some(Node::Directory(_)) => return FilesystemError::NotFound,
            None if dir.is_empty() => return FilesystemError::NotFound,
            None => dir = parent(dir),
        }
    }
}

/// The directory a new entry at `path` goes into must already be one.
fn check_parent(nodes: &BTreeMap<String, Node>, path: &str) -> Result<(), FilesystemError> {
    match nodes.get(parent(path)) {
        Some(Node::Directory(_)) => Ok(()),
        Some(Node::File(_)) => Err(FilesystemError::NotADirectory),
        None => Err(missing(nodes, parent(path))),
    }
}

fn has_children(nodes: &BTreeMap<String, Node>, dir: &str) -> bool {
    nodes.keys().any(|key| is_beneath(key, dir))
}

impl Filesystem for Memory {
    fn open(&self, path: &str, options: &OpenOptions) -> Result<Box<dyn File>, FilesystemError> {
        let mut nodes = self.nodes();
        let data = match nodes.get(path) {
            Some(Node::Directory(_)) => return Err(FilesystemError::IsADirectory),
            Some(Node::File(_)) if options.create_new => {
                return Err(FilesystemError::AlreadyExists)
            }
            Some(Node::File(data)) => {
                if options.truncate {
                    lock(data).resize(0)?;
                }
                data.clone()
            }
            None if options.create || options.create_new => {
                check_parent(&nodes, path)?;
                let data = Content::create(&self.space)?;
                nodes.insert(path.to_string(), Node::File(data.clone()));
                data
            }
            None => return Err(missing(&nodes, path)),
        };
        Ok(Box::new(MemoryFile { data }))
    }

    fn metadata(&self, path: &str) -> Result<Metadata, FilesystemError> {
        let nodes = self.nodes();
        match nodes.get(path) {
            Some(Node::File(data)) => {
                Ok(Metadata::new(FileType::File, lock(data).bytes.len() as u64))
            }
            Some(&Node::Directory(ino)) => {
                Ok(Metadata::new(FileType::Directory, 0).with_identity(self.dev, ino))
            }
            None => Err(missing(&nodes, path)),
        }
    }

    fn read_dir(&self, path: &str) -> Result<Vec<DirEntry>, FilesystemError> {
        let nodes = self.nodes();
        match nodes.get(path) {
            Some(Node::Directory(_)) => {}
            Some(Node::File(_)) => return Err(FilesystemError::NotADirectory),
            None => return Err(missing(&nodes, path)),
        }
        Ok(nodes
            .iter()
            .filter(|(key, _)| !key.is_empty() && parent(key) == path)
            .map(|(key, node)| {
                DirEntry::new(key.rsplit('/').next().unwrap_or(key), node.file_type())
            })
            .collect())
    }

    fn create_dir(&self, path: &str) -> Result<(), FilesystemError> {
        let mut nodes = self.nodes();
        if nodes.contains_key(path) {
            return Err(FilesystemError::AlreadyExists);
        }
        check_parent(&nodes, path)?;
        self.space.inodes.take(1)?;
        nodes.insert(path.to_string(), self.new_directory());
        Ok(())
    }

    fn remove_dir(&self, path: &str) -> Result<(), FilesystemError> {
        let mut nodes = self.nodes();
        match nodes.get(path) {
            Some(Node::File(_)) => Err(FilesystemError::NotADirectory),
            Some(Node::Directory(_)) if has_children(&nodes, path) => {
                Err(FilesystemError::DirectoryNotEmpty)
            }
            Some(Node::Directory(_)) => {
                nodes.remove(path);
                self.space.inodes.give(1);
                Ok(())
            }
            None => Err(missing(&nodes, path)),
        }
    }

    fn remove_file(&self, path: &str) -> Result<(), FilesystemError> {
        let mut nodes = self.nodes();
        match nodes.get(path) {
            Some(Node::File(_)) => {
                nodes.remove(path);
                Ok(())
            }
            Some(Node::Directory(_)) => Err(FilesystemError::IsADirectory),
            None => Err(missing(&nodes, path)),
        }
    }

    fn rename(&self, from: &str, to: &str, rename: Rename) -> Result<(), FilesystemError> {
        let mut nodes = self.nodes();
        let moving = match nodes.get(from) {
            Some(node) => node.clone(),
            None => return Err(missing(&nodes, from)),
        };
        if from == to {
            return Ok(());
        }
        rename.check(moving.file_type())?;
        if is_beneath(to, from) {
            return Err(beneath_itself(moving.file_type()));
        }
        check_parent(&nodes, to)?;
        match (&moving, nodes.get(to)) {
            (Node::File(_), Some(Node::Directory(_))) => return Err(FilesystemError::IsADirectory),
            (Node::Directory(_), Some(Node::File(_))) => {
                return Err(FilesystemError::NotADirectory)
            }
            (Node::Directory(_), Some(Node::Directory(_))) if has_children(&nodes, to) => {
                return Err(FilesystemError::DirectoryNotEmpty)
            }
            _ => {}
        }
        let beneath: Vec<String> = nodes
            .keys()
            .filter(|key| is_beneath(key, from))
            .cloned()
            .collect();
        nodes.remove(from);
        if let Some(Node::Directory(_)) = nodes.insert(to.to_string(), moving) {
            self.space.inodes.give(1);
        }
        for key in beneath {
            if let Some(node) = nodes.remove(&key) {
                nodes.insert(format!("{to}{}", &key[from.len()..]), node);
            }
        }
        Ok(())
    }
}

fn lock(data: &Mutex<Content>) -> MutexGuard<'_, Content> {
    data.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn write(data: &mut Content, buf: &[u8], start: usize) -> Result<usize, FilesystemError> {
    let end = start
        .checked_add(buf.len())
        .ok_or(FilesystemError::InvalidInput)?;
    if data.bytes.len() < end {
        data.resize(end)?;
    }
    data.bytes[start..end].copy_from_slice(buf);
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
        Ok(Metadata::new(
            FileType::File,
            lock(&self.data).bytes.len() as u64,
        ))
    }

    fn set_len(&mut self, len: u64) -> Result<(), FilesystemError> {
        let len = usize::try_from(len).map_err(|_| FilesystemError::InvalidInput)?;
        lock(&self.data).resize(len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_all(fs: &dyn Filesystem, path: &str) -> Vec<u8> {
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
        let memory: Memory = [("/config/app.yml", "name: kobako")].into_iter().collect();
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
        let memory = Memory::new();
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

    // @behavior FS-003
    #[test]
    fn every_invocation_shares_the_one_memory() {
        assert!(
            Memory::new().for_invocation().is_none(),
            "Memory::for_invocation must hand every Invocation the same Memory"
        );
    }

    #[test]
    fn a_directory_with_entries_is_not_removed() {
        let memory: Memory = [("dir/file", "x")].into_iter().collect();
        assert_eq!(
            memory.remove_dir("dir"),
            Err(FilesystemError::DirectoryNotEmpty)
        );
        assert_eq!(memory.remove_file("dir/file"), Ok(()));
        assert_eq!(memory.remove_dir("dir"), Ok(()));
        assert_eq!(memory.metadata("dir"), Err(FilesystemError::NotFound));
    }

    #[test]
    fn a_renamed_directory_carries_its_entries() {
        let memory: Memory = [("old/inner/file", "x")].into_iter().collect();
        memory
            .rename("old", "new", Rename::Any)
            .expect("the rename must succeed");
        assert_eq!(read_all(&memory, "new/inner/file"), b"x");
        assert_eq!(memory.metadata("old"), Err(FilesystemError::NotFound));
    }

    fn writable(fs: &dyn Filesystem, path: &str) -> Box<dyn File> {
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
        let memory = memory.with_limit(Some(4));
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

    // @behavior FS-023
    #[test]
    fn a_memory_holds_its_default_limit() {
        let memory = Memory::new();
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
        let memory = Memory::new().with_limit(None);
        assert_eq!(
            writable(&memory, "file").set_len(Memory::DEFAULT_LIMIT as u64 + 1),
            Ok(()),
            "a Memory given no limit through Memory::with_limit(None) must grow past 64 MiB"
        );
    }

    // @behavior FS-025
    #[test]
    fn a_removed_file_returns_its_space() {
        let memory = Memory::new().with_limit(Some(4));
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
        let memory = memory.with_limit(None);
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
        let memory: Memory = [("file", "x")].into_iter().collect();
        assert_eq!(
            memory.metadata("file/inner"),
            Err(FilesystemError::NotADirectory)
        );
    }

    fn create(fs: &dyn Filesystem, path: &str) -> Result<(), FilesystemError> {
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
        let memory = Memory::new().with_inodes(Some(3));
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
        let memory = Memory::new().with_limit(Some(2 * 4096));
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
        let memory: Memory = [("log", "a")].into_iter().collect();
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
        let memory: Memory = [("dir/inner", "x"), ("file", "y")].into_iter().collect();
        assert_eq!(
            (
                memory.rename("dir", "moved", Rename::File),
                memory.rename("file", "moved", Rename::Directory),
                memory.metadata("dir/inner").map(|m| m.file_type),
                memory.metadata("file").map(|m| m.file_type),
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
        let memory = Memory::new().with_inodes(Some(3));
        memory
            .create_dir("a")
            .expect("the first directory must fit");
        memory
            .create_dir("b")
            .expect("the second directory must fit");
        memory.remove_dir("a").expect("the directory must remove");
        let after_removal = memory.create_dir("c");
        memory
            .rename("c", "b", Rename::Any)
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
        let memory = Memory::new().with_inodes(Some(2));
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
        let memory = Memory::new().metadata("").expect("the root must stand").dev;
        assert!(
            [first, second, memory].iter().all(|dev| dev >> 63 == 1)
                && first != second
                && memory != first
                && memory != second,
            "devices from Metadata::unique_device and a Memory's root must each set the top bit \
             and differ from one another"
        );
    }

    // @behavior FS-070
    #[test]
    fn a_memory_gives_each_directory_it_makes_an_identity_of_its_own() {
        let memory = Memory::new();
        memory
            .create_dir("dir")
            .expect("the directory must be made");
        let identity = |path| {
            let metadata = memory.metadata(path).expect("the directory must stand");
            (metadata.dev, metadata.ino)
        };
        let before = identity("dir");
        memory
            .rename("dir", "moved", Rename::Any)
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
