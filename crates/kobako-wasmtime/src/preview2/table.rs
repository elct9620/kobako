//! The resource table preview2 hands the guest its handles from, as
//! wasmtime-wasi's `ResourceTable` is. Each entry counts against the same
//! `OpenFiles` as preview1's descriptors, since a socket is a file.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Instant;

use kobako_runtime::net::{Namespace, TcpSock};
use kobako_runtime::vfs::{File, FilesystemError, OpenFiles, PollMask};
use kobako_runtime::wait::PollTable;

/// One socket file the guest's `tcp-socket`, `input-stream`, and
/// `output-stream` all view, as three handles on one `struct file`; the last
/// view dropped releases the socket.
#[derive(Clone)]
pub(crate) struct SocketFile {
    pub(crate) sock: Arc<TcpSock>,
    file: Arc<Mutex<File>>,
}

impl SocketFile {
    pub(crate) fn new(sock: Arc<TcpSock>, file: File) -> Self {
        Self {
            sock,
            file: Arc::new(Mutex::new(file)),
        }
    }

    /// The file, which a read or a write takes `&mut`, so the views share it
    /// behind a lock.
    pub(crate) fn file(&self) -> MutexGuard<'_, File> {
        self.file.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// What one handle stands for.
pub(crate) enum Resource {
    /// The one network a Sandbox's Grants make, as `instance-network` hands it
    /// out.
    Network,
    Pollable(Pollable),
    TcpSocket(TcpSocket),
    InputStream(SocketFile),
    OutputStream(SocketFile),
    /// A stream's failure, as `wasi:io/error` hands one out.
    Error(FilesystemError),
}

/// Where a TCP socket stands, as WASI's state machine walks it.
pub(crate) enum TcpSocket {
    Unbound,
    Connecting(SocketFile),
    Connected(SocketFile),
    /// A connect that failed, after which the socket takes no other, as
    /// `tcp.wit` closes it.
    Closed,
}

/// What a pollable waits for.
#[derive(Clone)]
pub(crate) enum Pollable {
    /// A clock, ready once `Instant` passes; one subscribed on frozen clocks is
    /// ready at once, as a wait with no host-side producer ends at once.
    Clock(Option<Instant>),
    /// A socket file, ready once its poll finds `PollMask`, or an error or a
    /// hang-up.
    Socket(SocketFile, PollMask),
    /// Something that is always ready, as an unconnected socket is.
    Ready,
}

impl Pollable {
    /// Whether it is ready, registering on `table` for a poll that will sleep.
    pub(crate) fn ready(&self, table: Option<&mut PollTable>) -> bool {
        match self {
            Pollable::Clock(at) => at.is_none_or(|at| Instant::now() >= at),
            Pollable::Socket(socket, mask) => {
                let found = socket.file().poll(table);
                found.contains(*mask)
                    || found.contains(PollMask::ERR)
                    || found.contains(PollMask::HUP)
            }
            Pollable::Ready => true,
        }
    }

    /// The soonest this could become ready by itself.
    pub(crate) fn due(&self) -> Option<Instant> {
        match self {
            Pollable::Clock(at) => *at,
            Pollable::Socket(..) | Pollable::Ready => None,
        }
    }
}

pub(crate) struct Table {
    entries: BTreeMap<u32, Resource>,
    next: u32,
    open_files: OpenFiles,
    network: Option<Namespace>,
}

impl Table {
    /// A table holding nothing, counting against `open_files`, with no
    /// network until one is granted.
    pub(crate) fn new(open_files: OpenFiles) -> Self {
        Self {
            entries: BTreeMap::new(),
            next: 1,
            open_files,
            network: None,
        }
    }

    /// Give the guest `network`, as an Invocation's network Grants make it.
    pub(crate) fn grant(&mut self, network: Namespace) {
        self.network = Some(network);
    }

    pub(crate) fn network(&self) -> Option<&Namespace> {
        self.network.as_ref()
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

    pub(crate) fn get_mut(&mut self, handle: u32) -> wasmtime::Result<&mut Resource> {
        self.entries
            .get_mut(&handle)
            .ok_or_else(|| wasmtime::Error::msg("the guest named a handle it does not hold"))
    }

    pub(crate) fn pollable(&self, handle: u32) -> wasmtime::Result<Pollable> {
        match self.get(handle)? {
            Resource::Pollable(pollable) => Ok(pollable.clone()),
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
