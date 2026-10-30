//! The guest's descriptor table: what each preview1 file descriptor
//! stands for within one invocation.
//!
//! kobako answers every preview1 call that reads this table, so the guest
//! sees one table and no call falls through to a second one that
//! disagrees. Numbering follows wasmtime-wasi's: a closed number is reused
//! before a fresh one is taken.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use kobako_runtime::filesystem::{DirEntry, FileOperations};
use kobako_runtime::mount::Mount;
use kobako_runtime::profile::Profile;
use kobako_runtime::vfs::Path;
use wasmtime_wasi::p2::pipe::{MemoryInputPipe, MemoryOutputPipe};

/// What one descriptor stands for.
pub(crate) enum Descriptor {
    /// Descriptor 0, carrying the Preamble.
    Stdin(MemoryInputPipe),
    /// Descriptor 1 or 2, a capture pipe the guest writes a Channel into.
    Channel(MemoryOutputPipe),
    /// A directory beneath a Mount; a preopen is the Mount's own root.
    Directory(OpenDirectory),
    File(OpenFile),
}

pub(crate) struct OpenDirectory {
    pub(crate) path: Path,
    pub(crate) preopen: bool,
    /// The entries a listing began with, held until it reaches its end, so
    /// each later call reads on without listing the directory again.
    pub(crate) listing: Option<Vec<DirEntry>>,
}

pub(crate) struct OpenFile {
    pub(crate) file: Box<dyn FileOperations>,
    pub(crate) position: u64,
    pub(crate) append: bool,
    pub(crate) readable: bool,
    pub(crate) writable: bool,
    /// The Mount it was opened beneath, whose permission to write answers a
    /// wait to write and a resize, as wasmtime-wasi answers both by its
    /// preopen's permissions.
    pub(crate) mount: Arc<Mount>,
}

pub(crate) struct Descriptors {
    used: BTreeMap<u32, Descriptor>,
    free: BTreeSet<u32>,
    limit: Option<usize>,
}

impl Descriptors {
    /// The table every invocation starts from: stdin, then the two
    /// Channels, then one preopen per Mount from descriptor 3. It holds at
    /// most `limit` descriptors, those included, as a process's
    /// `RLIMIT_NOFILE` counts every one it holds. Each Mount is seen as an
    /// invocation running under `profile` sees it.
    pub(crate) fn new(
        stdin: MemoryInputPipe,
        stdout: MemoryOutputPipe,
        stderr: MemoryOutputPipe,
        mounts: Vec<Mount>,
        limit: Option<usize>,
        profile: Profile,
    ) -> Self {
        let mut used = BTreeMap::from([
            (0, Descriptor::Stdin(stdin)),
            (1, Descriptor::Channel(stdout)),
            (2, Descriptor::Channel(stderr)),
        ]);
        for (fd, mount) in (3..).zip(mounts) {
            used.insert(
                fd,
                Descriptor::Directory(OpenDirectory {
                    path: Path::root(mount, profile),
                    preopen: true,
                    listing: None,
                }),
            );
        }
        Self {
            used,
            free: BTreeSet::new(),
            limit,
        }
    }

    pub(crate) fn get(&self, fd: u32) -> Option<&Descriptor> {
        self.used.get(&fd)
    }

    pub(crate) fn get_mut(&mut self, fd: u32) -> Option<&mut Descriptor> {
        self.used.get_mut(&fd)
    }

    /// Take `fd` out of the table, freeing its number for reuse.
    pub(crate) fn remove(&mut self, fd: u32) -> Option<Descriptor> {
        let descriptor = self.used.remove(&fd)?;
        self.free.insert(fd);
        Some(descriptor)
    }

    /// Place `descriptor` at `fd`, a number the caller has just freed.
    pub(crate) fn place(&mut self, fd: u32, descriptor: Descriptor) {
        self.free.remove(&fd);
        self.used.insert(fd, descriptor);
    }

    /// Whether the table holds as many descriptors as it may.
    pub(crate) fn is_full(&self) -> bool {
        self.limit.is_some_and(|limit| self.used.len() >= limit)
    }

    /// Add `descriptor` under the highest freed number, or the one past
    /// the highest in use; `None` when the table is full.
    pub(crate) fn push(&mut self, descriptor: Descriptor) -> Option<u32> {
        if self.is_full() {
            return None;
        }
        let fd = match self.free.pop_last() {
            Some(fd) => fd,
            None => match self.used.last_key_value() {
                Some((last, _)) => last.checked_add(1)?,
                None => 0,
            },
        };
        self.used.insert(fd, descriptor);
        Some(fd)
    }
}
