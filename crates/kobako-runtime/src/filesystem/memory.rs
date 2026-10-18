//! `Memory`: a Filesystem held in memory, living as long as the Host
//! App's object does.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use super::path::{is_beneath, parent, seeded};
use super::{DirEntry, File, Filesystem, FilesystemError, Kind, Metadata, OpenOptions};

/// Files and directories held in memory. Every Invocation it is handed to
/// shares it, so what one writes the next reads. What its files hold
/// together is bounded, as a tmpfs mount is, so a guest cannot grow it
/// past what the Host App allowed.
pub struct Memory {
    nodes: Mutex<BTreeMap<String, Node>>,
    space: Arc<Space>,
}

#[derive(Clone)]
enum Node {
    File(Arc<Mutex<Content>>),
    Directory,
}

/// The bytes a Memory's files hold together, and the most they may.
struct Space {
    limit: AtomicUsize,
    used: AtomicUsize,
}

impl Space {
    /// Count `bytes` more, refusing when they would pass the limit.
    fn take(&self, bytes: usize) -> Result<(), FilesystemError> {
        let limit = self.limit.load(Ordering::Relaxed);
        self.used
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |used| {
                used.checked_add(bytes).filter(|&total| total <= limit)
            })
            .map(drop)
            .map_err(|_| FilesystemError::NoSpace)
    }

    fn give(&self, bytes: usize) {
        self.used.fetch_sub(bytes, Ordering::Relaxed);
    }
}

/// One file's bytes, counted against its Memory's space for as long as
/// they live, which outlasts the file's name while a handle holds them.
struct Content {
    bytes: Vec<u8>,
    space: Arc<Space>,
}

impl Content {
    fn new(bytes: Vec<u8>, space: &Arc<Space>) -> Arc<Mutex<Self>> {
        space.used.fetch_add(bytes.len(), Ordering::Relaxed);
        Arc::new(Mutex::new(Self {
            bytes,
            space: space.clone(),
        }))
    }

    /// Resize to `len`, refusing a growth past the space left or past what
    /// can be allocated, rather than aborting the host process.
    fn resize(&mut self, len: usize) -> Result<(), FilesystemError> {
        let growth = len.saturating_sub(self.bytes.len());
        self.space.take(growth)?;
        if self.bytes.try_reserve(growth).is_err() {
            self.space.give(growth);
            return Err(FilesystemError::NoSpace);
        }
        self.space.give(self.bytes.len().saturating_sub(len));
        self.bytes.resize(len, 0);
        Ok(())
    }
}

impl Drop for Content {
    fn drop(&mut self) {
        self.space.give(self.bytes.len());
    }
}

impl Memory {
    /// The most bytes a Memory's files hold unless given a limit of their
    /// own: the size Docker gives a `--tmpfs` mount.
    pub const DEFAULT_LIMIT: usize = 64 * 1024 * 1024;

    pub fn new() -> Self {
        Self {
            nodes: Mutex::new(BTreeMap::from([(String::new(), Node::Directory)])),
            space: Arc::new(Space {
                limit: AtomicUsize::new(Self::DEFAULT_LIMIT),
                used: AtomicUsize::new(0),
            }),
        }
    }

    /// This Memory with its files holding at most `limit` bytes together,
    /// or without bound for `None`. Files it was seeded with count, but
    /// are never refused.
    pub fn with_limit(self, limit: Option<usize>) -> Self {
        self.space
            .limit
            .store(limit.unwrap_or(usize::MAX), Ordering::Relaxed);
        self
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

/// A Memory seeded with files, each `(path, contents)`, its directories
/// created along the way.
impl<P: AsRef<str>, C: Into<Vec<u8>>> FromIterator<(P, C)> for Memory {
    fn from_iter<I: IntoIterator<Item = (P, C)>>(files: I) -> Self {
        let memory = Self::new();
        {
            let mut nodes = memory.nodes();
            for (path, contents) in files {
                let path = seeded(path.as_ref());
                for (end, _) in path.match_indices('/') {
                    nodes.insert(path[..end].to_string(), Node::Directory);
                }
                nodes.insert(
                    path,
                    Node::File(Content::new(contents.into(), &memory.space)),
                );
            }
        }
        memory
    }
}

/// Why `path` names nothing: a file standing where a directory above it
/// should be, or simply no entry.
fn missing(nodes: &BTreeMap<String, Node>, path: &str) -> FilesystemError {
    let mut dir = parent(path);
    loop {
        match nodes.get(dir) {
            Some(Node::File(_)) => return FilesystemError::NotADirectory,
            Some(Node::Directory) => return FilesystemError::NotFound,
            None if dir.is_empty() => return FilesystemError::NotFound,
            None => dir = parent(dir),
        }
    }
}

/// The directory a new entry at `path` goes into must already be one.
fn check_parent(nodes: &BTreeMap<String, Node>, path: &str) -> Result<(), FilesystemError> {
    match nodes.get(parent(path)) {
        Some(Node::Directory) => Ok(()),
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
            Some(Node::Directory) => return Err(FilesystemError::IsADirectory),
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
                let data = Content::new(Vec::new(), &self.space);
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
            Some(Node::File(data)) => Ok(Metadata {
                kind: Kind::File,
                len: lock(data).bytes.len() as u64,
            }),
            Some(Node::Directory) => Ok(Metadata {
                kind: Kind::Directory,
                len: 0,
            }),
            None => Err(missing(&nodes, path)),
        }
    }

    fn read_dir(&self, path: &str) -> Result<Vec<DirEntry>, FilesystemError> {
        let nodes = self.nodes();
        match nodes.get(path) {
            Some(Node::Directory) => {}
            Some(Node::File(_)) => return Err(FilesystemError::NotADirectory),
            None => return Err(missing(&nodes, path)),
        }
        Ok(nodes
            .iter()
            .filter(|(key, _)| !key.is_empty() && parent(key) == path)
            .map(|(key, node)| DirEntry {
                name: key.rsplit('/').next().unwrap_or(key).to_string(),
                kind: match node {
                    Node::File(_) => Kind::File,
                    Node::Directory => Kind::Directory,
                },
            })
            .collect())
    }

    fn create_dir(&self, path: &str) -> Result<(), FilesystemError> {
        let mut nodes = self.nodes();
        if nodes.contains_key(path) {
            return Err(FilesystemError::AlreadyExists);
        }
        check_parent(&nodes, path)?;
        nodes.insert(path.to_string(), Node::Directory);
        Ok(())
    }

    fn remove_dir(&self, path: &str) -> Result<(), FilesystemError> {
        let mut nodes = self.nodes();
        match nodes.get(path) {
            Some(Node::File(_)) => Err(FilesystemError::NotADirectory),
            Some(Node::Directory) if has_children(&nodes, path) => {
                Err(FilesystemError::DirectoryNotEmpty)
            }
            Some(Node::Directory) => {
                nodes.remove(path);
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
            Some(Node::Directory) => Err(FilesystemError::IsADirectory),
            None => Err(missing(&nodes, path)),
        }
    }

    fn rename(&self, from: &str, to: &str) -> Result<(), FilesystemError> {
        let mut nodes = self.nodes();
        let moving = match nodes.get(from) {
            Some(node) => node.clone(),
            None => return Err(missing(&nodes, from)),
        };
        if from == to {
            return Ok(());
        }
        if is_beneath(to, from) {
            return Err(FilesystemError::InvalidInput);
        }
        check_parent(&nodes, to)?;
        match (&moving, nodes.get(to)) {
            (Node::File(_), Some(Node::Directory)) => return Err(FilesystemError::IsADirectory),
            (Node::Directory, Some(Node::File(_))) => return Err(FilesystemError::NotADirectory),
            (Node::Directory, Some(Node::Directory)) if has_children(&nodes, to) => {
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
        nodes.insert(to.to_string(), moving);
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
        let mut data = lock(&self.data);
        let start = usize::try_from(offset).map_err(|_| FilesystemError::InvalidInput)?;
        let end = start
            .checked_add(buf.len())
            .ok_or(FilesystemError::InvalidInput)?;
        if data.bytes.len() < end {
            data.resize(end)?;
        }
        data.bytes[start..end].copy_from_slice(buf);
        Ok(buf.len())
    }

    fn metadata(&self) -> Result<Metadata, FilesystemError> {
        Ok(Metadata {
            kind: Kind::File,
            len: lock(&self.data).bytes.len() as u64,
        })
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
            .rename("old", "new")
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
            (Err(FilesystemError::NoSpace), b"x".to_vec()),
            "a write growing a Memory's files past its limit through MemoryFile::write_at must \
             answer NoSpace and leave the file as it was"
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
            (Ok(()), Err(FilesystemError::NoSpace)),
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
                Err(FilesystemError::NoSpace),
                Err(FilesystemError::NoSpace),
                b"x".to_vec()
            ),
            "a length no allocator can hand out through MemoryFile::set_len and write_at must \
             answer NoSpace rather than abort"
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
}
