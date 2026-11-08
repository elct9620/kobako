//! The resource table preview2 hands the guest its handles from, as
//! wasmtime-wasi's `ResourceTable` is. Each entry counts against the same
//! `OpenFiles` as preview1's descriptors, since a socket is a file.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Instant;

use kobako_runtime::net::{
    Exchange, NameLookup, Namespace, RequestBody, TcpSock, UdpSock, SOCKET_BUFFER,
};
use kobako_runtime::vfs::{File, FilesystemError, OpenFiles, PollMask};
use kobako_runtime::wait::PollTable;

/// One socket file every view of a guest socket shares, as handles on one
/// `struct file` share one `struct socket`: the protocol's half is `S`, and
/// the last view dropped releases the socket.
pub(crate) struct SocketFile<S> {
    pub(crate) sock: Arc<S>,
    file: Arc<Mutex<File>>,
    /// The peer as the guest named it, a virtual address standing for its
    /// name, as cgroup `getpeername4` maps a real peer back.
    pub(crate) peer: SocketAddr,
}

impl<S> Clone for SocketFile<S> {
    fn clone(&self) -> Self {
        Self {
            sock: self.sock.clone(),
            file: self.file.clone(),
            peer: self.peer,
        }
    }
}

impl<S> SocketFile<S> {
    pub(crate) fn new(sock: Arc<S>, file: File, peer: SocketAddr) -> Self {
        Self {
            sock,
            file: Arc::new(Mutex::new(file)),
            peer,
        }
    }

    /// The file, which a read or a write takes `&mut`, so the views share it
    /// behind a lock.
    pub(crate) fn file(&self) -> MutexGuard<'_, File> {
        self.file.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// What an `input-stream` or an `output-stream` reads or writes: a socket file,
/// a response's body, or a request's body.
#[derive(Clone)]
pub(crate) enum Stream {
    Socket(SocketFile<TcpSock>),
    Response(Arc<Exchange>),
    Request(Arc<RequestBody>),
}

impl Stream {
    /// Read what the stream holds now; `WouldBlock` while it holds nothing yet.
    pub(crate) fn read(&self, buf: &mut [u8]) -> Result<usize, FilesystemError> {
        match self {
            Stream::Socket(socket) => socket.file().read(buf),
            Stream::Response(exchange) => exchange.read(buf),
            Stream::Request(_) => Err(FilesystemError::BadDescriptor),
        }
    }

    /// Write what the stream takes now.
    pub(crate) fn write(&self, buf: &[u8]) -> Result<usize, FilesystemError> {
        match self {
            Stream::Socket(socket) => socket.file().write(buf),
            Stream::Request(body) => body.write(buf),
            Stream::Response(_) => Err(FilesystemError::BadDescriptor),
        }
    }

    /// How many bytes a write takes now.
    pub(crate) fn room(&self) -> usize {
        match self {
            Stream::Socket(socket) => socket.sock.send_room(),
            Stream::Request(_) | Stream::Response(_) => SOCKET_BUFFER,
        }
    }

    /// Whether what was written has left for the host, so a flush is done.
    pub(crate) fn drained(&self) -> bool {
        self.room() == SOCKET_BUFFER
    }

    /// What a poll finds, registering on `table` for a poll that will sleep.
    pub(crate) fn poll(&self, table: Option<&mut PollTable>) -> PollMask {
        match self {
            Stream::Socket(socket) => socket.file().poll(table),
            Stream::Response(exchange) => exchange.poll(table),
            Stream::Request(_) => PollMask::OUT,
        }
    }
}

/// Where a UDP socket stands, as WASI's state machine walks it.
pub(crate) enum UdpSocket {
    Unbound,
    Binding,
    Bound,
    /// One view of the socket file, so the host socket lives while the
    /// udp-socket does, as each of a file's views holds it.
    Streaming {
        _file: SocketFile<UdpSock>,
    },
}

/// What one handle stands for.
pub(crate) enum Resource {
    /// The one network a Sandbox's Grants make, as `instance-network` hands it
    /// out.
    Network,
    Pollable(Pollable),
    TcpSocket(TcpSocket),
    UdpSocket(UdpSocket),
    /// An incoming or outgoing datagram stream.
    Datagrams(SocketFile<UdpSock>),
    InputStream(Stream),
    OutputStream(Stream),
    /// One of `wasi:http`'s resources.
    Http(super::http::Held),
    /// A stream's failure, as `wasi:io/error` hands one out.
    Error(FilesystemError),
    /// A lookup, and whether its one address was handed out.
    ResolveStream(Arc<NameLookup>, bool),
}

/// Where a TCP socket stands, as WASI's state machine walks it.
pub(crate) enum TcpSocket {
    Unbound,
    Connecting(SocketFile<TcpSock>),
    Connected(SocketFile<TcpSock>),
    /// A connect that failed, after which the socket takes no other, as
    /// `tcp.wit` closes it.
    Closed,
}

/// What a pollable waits for.
#[derive(Clone)]
pub(crate) enum Pollable {
    /// A clock, ready once `at` passes. One subscribed on frozen clocks is
    /// ready at once in a wait with no host-side producer, which then ends at
    /// once; beside a producer it elapses in host time from when it was made.
    Clock { at: Instant, frozen: bool },
    /// A stream, ready once its poll finds `PollMask`, or an error or a
    /// hang-up.
    Stream(Stream, PollMask),
    /// A lookup, ready once it answers.
    Lookup(Arc<NameLookup>),
    /// A request in flight, ready once its response's head arrives.
    Response(Arc<Exchange>),
    /// A datagram stream, ready once its poll finds `PollMask`, or an error.
    Datagrams(SocketFile<UdpSock>, PollMask),
    /// Something that is always ready, as an unconnected socket is.
    Ready,
}

impl Pollable {
    /// Whether it is ready, in a wait that holds a host-side producer when
    /// `producer` says so, registering on `table` for a poll that will sleep.
    pub(crate) fn ready(&self, table: Option<&mut PollTable>, producer: bool) -> bool {
        match self {
            Pollable::Clock { at, frozen } => (*frozen && !producer) || Instant::now() >= *at,
            Pollable::Stream(stream, mask) => {
                let found = stream.poll(table);
                found.contains(*mask)
                    || found.contains(PollMask::ERR)
                    || found.contains(PollMask::HUP)
            }
            Pollable::Lookup(lookup) => lookup.poll(table),
            Pollable::Response(exchange) => exchange.poll(table).contains(PollMask::OUT),
            Pollable::Datagrams(datagrams, mask) => {
                let found = datagrams.file().poll(table);
                found.contains(*mask) || found.contains(PollMask::ERR)
            }
            Pollable::Ready => true,
        }
    }

    /// The soonest this could become ready by itself.
    pub(crate) fn due(&self) -> Option<Instant> {
        match self {
            Pollable::Clock { at, .. } => Some(*at),
            _ => None,
        }
    }

    /// Whether a host-side producer makes this ready, as a socket's peer, a
    /// resolver, or a server does.
    pub(crate) fn is_producer(&self) -> bool {
        matches!(
            self,
            Pollable::Stream(..)
                | Pollable::Lookup(_)
                | Pollable::Response(_)
                | Pollable::Datagrams(..)
        )
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
