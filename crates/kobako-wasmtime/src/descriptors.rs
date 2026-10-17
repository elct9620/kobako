//! The guest's descriptor table: what each preview1 file descriptor
//! stands for within one invocation.
//!
//! kobako answers every preview1 call that reads this table, so the guest
//! sees one table and no call falls through to a second one that
//! disagrees.

use std::collections::BTreeMap;

use wasmtime_wasi::p2::pipe::{MemoryInputPipe, MemoryOutputPipe};

/// What one descriptor stands for.
pub(crate) enum Descriptor {
    /// Descriptor 0, carrying the Preamble.
    Stdin(MemoryInputPipe),
    /// Descriptor 1 or 2, a capture pipe the guest writes a Channel into.
    Channel(MemoryOutputPipe),
}

pub(crate) struct Descriptors {
    used: BTreeMap<u32, Descriptor>,
}

impl Descriptors {
    /// The table every invocation starts from: stdin, then the two
    /// Channels.
    pub(crate) fn new(
        stdin: MemoryInputPipe,
        stdout: MemoryOutputPipe,
        stderr: MemoryOutputPipe,
    ) -> Self {
        let used = BTreeMap::from([
            (0, Descriptor::Stdin(stdin)),
            (1, Descriptor::Channel(stdout)),
            (2, Descriptor::Channel(stderr)),
        ]);
        Self { used }
    }

    pub(crate) fn get(&self, fd: u32) -> Option<&Descriptor> {
        self.used.get(&fd)
    }

    pub(crate) fn get_mut(&mut self, fd: u32) -> Option<&mut Descriptor> {
        self.used.get_mut(&fd)
    }

    pub(crate) fn remove(&mut self, fd: u32) -> Option<Descriptor> {
        self.used.remove(&fd)
    }

    /// Place `descriptor` at `fd`, a number the caller has just closed.
    pub(crate) fn place(&mut self, fd: u32, descriptor: Descriptor) {
        self.used.insert(fd, descriptor);
    }
}
