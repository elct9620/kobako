//! `Temporary`: a Filesystem held in memory that every Invocation starts
//! empty.

use std::sync::Arc;

use super::memory::Memory;
use super::{DirEntry, File, Filesystem, FilesystemError, Metadata, OpenOptions};

/// Files and directories held in memory for one Invocation. The next one
/// is handed a new, empty Temporary in its place, so nothing one writes
/// reaches another. Each one holds as much as a Memory does.
pub struct Temporary {
    memory: Memory,
    limit: Option<usize>,
}

impl Temporary {
    pub fn new() -> Self {
        Self::default()
    }

    /// This Temporary, and every one handed out in its place, with its
    /// files holding at most `limit` bytes together, or without bound for
    /// `None`.
    pub fn with_limit(self, limit: Option<usize>) -> Self {
        Self {
            memory: self.memory.with_limit(limit),
            limit,
        }
    }
}

impl Default for Temporary {
    fn default() -> Self {
        Self {
            memory: Memory::new(),
            limit: Some(Memory::DEFAULT_LIMIT),
        }
    }
}

impl Filesystem for Temporary {
    fn open(&self, path: &str, options: &OpenOptions) -> Result<Box<dyn File>, FilesystemError> {
        self.memory.open(path, options)
    }

    fn metadata(&self, path: &str) -> Result<Metadata, FilesystemError> {
        self.memory.metadata(path)
    }

    fn read_dir(&self, path: &str) -> Result<Vec<DirEntry>, FilesystemError> {
        self.memory.read_dir(path)
    }

    fn create_dir(&self, path: &str) -> Result<(), FilesystemError> {
        self.memory.create_dir(path)
    }

    fn remove_dir(&self, path: &str) -> Result<(), FilesystemError> {
        self.memory.remove_dir(path)
    }

    fn remove_file(&self, path: &str) -> Result<(), FilesystemError> {
        self.memory.remove_file(path)
    }

    fn rename(&self, from: &str, to: &str) -> Result<(), FilesystemError> {
        self.memory.rename(from, to)
    }

    fn for_invocation(&self) -> Option<Arc<dyn Filesystem>> {
        Some(Arc::new(Self::new().with_limit(self.limit)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // @behavior FS-004
    #[test]
    fn a_temporary_starts_every_invocation_empty() {
        let temporary = Temporary::new();
        temporary
            .open(
                "scratch",
                &OpenOptions {
                    write: true,
                    create: true,
                    ..OpenOptions::default()
                },
            )
            .expect("a new file must be created")
            .write_at(b"gone", 0)
            .expect("the write must land");

        let next = temporary
            .for_invocation()
            .expect("a Temporary must hand the next Invocation a Filesystem of its own");
        assert_eq!(
            next.read_dir("").expect("the root must list"),
            Vec::new(),
            "Temporary::for_invocation must hand the next Invocation a Filesystem holding no file"
        );
    }

    // @behavior FS-027
    #[test]
    fn a_temporary_keeps_its_limit_in_every_invocation() {
        let next = Temporary::new()
            .with_limit(Some(4))
            .for_invocation()
            .expect("a Temporary must hand the next Invocation a Filesystem of its own");
        let written = next
            .open(
                "scratch",
                &OpenOptions {
                    write: true,
                    create: true,
                    ..OpenOptions::default()
                },
            )
            .expect("a new file must be created")
            .write_at(b"five!", 0);
        assert_eq!(
            written,
            Err(FilesystemError::NoSpace),
            "a write past a Temporary's limit in the Filesystem Temporary::for_invocation hands \
             the next Invocation must answer NoSpace"
        );
    }
}
