//! The resource table preview2 hands the guest its handles from, as
//! wasmtime-wasi's `ResourceTable` is. Each entry counts against the same
//! `OpenFiles` as preview1's descriptors, since a socket is a file.

use std::collections::BTreeMap;
use std::time::Instant;

use kobako_runtime::vfs::OpenFiles;

/// What one handle stands for.
pub(crate) enum Resource {
    /// The one network a Sandbox's Grants make, as `instance-network` hands it
    /// out.
    Network,
    Pollable(Pollable),
}

/// What a pollable waits for.
#[derive(Clone, Copy)]
pub(crate) enum Pollable {
    /// A clock, ready once `Instant` passes; one subscribed on frozen clocks is
    /// ready at once, as a wait with no host-side producer ends at once.
    Clock(Option<Instant>),
}

impl Pollable {
    pub(crate) fn ready(&self) -> bool {
        match self {
            Pollable::Clock(at) => at.is_none_or(|at| Instant::now() >= at),
        }
    }

    /// The soonest this could become ready by itself.
    pub(crate) fn due(&self) -> Option<Instant> {
        match self {
            Pollable::Clock(at) => *at,
        }
    }
}

pub(crate) struct Table {
    entries: BTreeMap<u32, Resource>,
    next: u32,
    open_files: OpenFiles,
}

impl Table {
    pub(crate) fn new(open_files: OpenFiles) -> Self {
        Self {
            entries: BTreeMap::new(),
            next: 1,
            open_files,
        }
    }

    /// Hand out a handle for `resource`, refused past the Invocation's open
    /// file limit.
    pub(crate) fn push(&mut self, resource: Resource) -> wasmtime::Result<u32> {
        self.open_files.open().map_err(|_| {
            wasmtime::Error::msg("the guest holds as many files and resources as its limit allows")
        })?;
        let handle = self.next;
        self.next += 1;
        self.entries.insert(handle, resource);
        Ok(handle)
    }

    pub(crate) fn get(&self, handle: u32) -> wasmtime::Result<&Resource> {
        self.entries
            .get(&handle)
            .ok_or_else(|| wasmtime::Error::msg("the guest named a handle it does not hold"))
    }

    pub(crate) fn pollable(&self, handle: u32) -> wasmtime::Result<Pollable> {
        match self.get(handle)? {
            Resource::Pollable(pollable) => Ok(*pollable),
            _ => Err(wasmtime::Error::msg(
                "the guest named a handle that is not a pollable",
            )),
        }
    }

    pub(crate) fn delete(&mut self, handle: u32) -> wasmtime::Result<Resource> {
        let resource = self
            .entries
            .remove(&handle)
            .ok_or_else(|| wasmtime::Error::msg("the guest dropped a handle it does not hold"))?;
        self.open_files.close();
        Ok(resource)
    }
}
