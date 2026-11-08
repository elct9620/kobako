//! What the Wasm Driver answers each preview2 import. Without a network Grant
//! the guest makes no socket, as WASI says a host without a network answers;
//! with one, each connect passes the Invocation's ruleset and dials through
//! its Protocol Family. The clock subscriptions, the streams, and `poll` wait
//! on the runtime's wait primitive.

use std::net::Shutdown;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use kobako_runtime::net::{
    sock_alloc_file, Namespace, ProtoOps, UdpSock, MAX_DATAGRAM, MAX_SOCKET_BUFFER,
};
use kobako_runtime::vfs::{File, FilesystemError, PollMask};
use kobako_runtime::wait::{do_poll, PollTable};

use super::bindings::wasi_clocks_monotonic_clock_0_2_12 as monotonic_clock;
use super::bindings::wasi_io_error_0_2_12::Error;
use super::bindings::wasi_io_poll_0_2_12::Pollable;
use super::bindings::wasi_io_streams_0_2_12::{InputStream, OutputStream, StreamError};
use super::bindings::wasi_sockets_ip_name_lookup_0_2_12::ResolveAddressStream;
use super::bindings::wasi_sockets_network_0_2_12::{
    ErrorCode, IpAddress, IpAddressFamily, IpSocketAddress, Network,
};
use super::bindings::wasi_sockets_tcp_0_2_12::{ShutdownType, TcpSocket};
use super::bindings::wasi_sockets_udp_0_2_12::{
    IncomingDatagram, IncomingDatagramStream, OutgoingDatagram, OutgoingDatagramStream, UdpSocket,
};
use super::bindings::{ClocksHost, IoHost, SocketsHost};
use super::sockets::{from_socket_addr, to_error_code, to_lookup_error_code, to_socket_addr};
use super::table::{self, SocketFile, Stream, TcpState, UdpState};
use crate::invocation::Invocation;
use crate::trap::TimeoutTrap;

/// The most one `blocking-write-and-flush` takes, past which `io.wit` says the
/// call traps.
const MAX_BLOCKING_WRITE: usize = 4096;

impl Invocation {
    /// A pollable on the guest's own clocks, `nanos` of host time from now;
    /// the wait holding it decides when it is due.
    fn subscribe_clock(&mut self, nanos: u64) -> wasmtime::Result<Pollable> {
        self.table().push(table::Pollable::Clock {
            at: Instant::now() + Duration::from_nanos(nanos),
        })
    }

    pub(super) fn table(&mut self) -> &mut table::Table {
        self.wasi_mut().table_mut()
    }

    /// Wait until `ready` answers, or trap as the deadline or an interrupt
    /// ends the run; the wait sleeps until the soonest pollable is due, or a
    /// queue `ready` registered on wakes.
    pub(crate) fn wait_for<T>(
        &self,
        due: Option<Instant>,
        mut ready: impl FnMut(Option<&mut PollTable>) -> Option<T>,
    ) -> wasmtime::Result<T> {
        if let Some(answer) = ready(None) {
            return Ok(answer);
        }
        loop {
            let timeout = due.map(|at| at.saturating_duration_since(Instant::now()));
            match do_poll(timeout, &mut ready) {
                Ok(Some(answer)) => return Ok(answer),
                Ok(None) => {
                    if let Some(answer) = ready(None) {
                        return Ok(answer);
                    }
                }
                Err(_) if self.deadline().is_some_and(|at| Instant::now() >= at) => {
                    return Err(wasmtime::Error::new(TimeoutTrap));
                }
                Err(_) => return Err(wasmtime::Error::msg("the guest's wait was interrupted")),
            }
        }
    }

    /// The handle is the capability to connect or look up, so it must name a
    /// network, as WASI's host checks it.
    fn ensure_network(&self, network: Network) -> wasmtime::Result<()> {
        self.wasi().table().get(network).map(drop)
    }

    /// The Invocation's network, which every socket is made in.
    fn network(&self) -> &Namespace {
        self.wasi()
            .table()
            .network()
            .expect("a socket is only made with a network")
    }

    /// The socket file of `sock`, opened as the Invocation's credentials.
    fn sock_alloc_file(&self, sock: Arc<dyn ProtoOps>) -> File {
        sock_alloc_file(sock, self.wasi().descriptors().cred())
    }

    fn subscribe_stream(&mut self, stream: Stream, mask: PollMask) -> wasmtime::Result<Pollable> {
        self.table()
            .push(table::Pollable::File(stream.file(), mask))
    }

    fn tcp(&mut self, handle: TcpSocket) -> wasmtime::Result<&mut table::TcpSocket> {
        self.table().get_mut(handle)
    }

    fn udp(&mut self, handle: UdpSocket) -> wasmtime::Result<&mut table::UdpSocket> {
        self.table().get_mut(handle)
    }

    /// The permit an outgoing datagram stream's last `check-send` gave.
    fn permit(&mut self, stream: OutgoingDatagramStream) -> wasmtime::Result<&mut u64> {
        Ok(&mut self.table().get_mut(stream)?.permit)
    }

    fn subscribe_datagrams(
        &mut self,
        datagrams: SocketFile<UdpSock>,
        mask: PollMask,
    ) -> wasmtime::Result<Pollable> {
        self.table()
            .push(table::Pollable::File(datagrams.get_file(), mask))
    }

    /// A stream's failure as WASI hands it out: the end as `closed`, anything
    /// else as an error the guest may read.
    fn push_stream_error(&mut self, err: FilesystemError) -> wasmtime::Result<StreamError> {
        match err {
            FilesystemError::BrokenPipe => Ok(StreamError::Closed),
            err => Ok(StreamError::LastOperationFailed(
                self.table().push(table::Error(err))?,
            )),
        }
    }

    /// Read what the stream holds, up to `len`, without waiting: nothing yet
    /// reads as an empty list, and the end as `closed`.
    fn read_now(
        &mut self,
        stream: &Stream,
        len: u64,
    ) -> wasmtime::Result<Result<Vec<u8>, StreamError>> {
        let mut buf = vec![0; len.min(MAX_SOCKET_BUFFER as u64) as usize];
        let read = stream.read(&mut buf);
        match read {
            Ok(0) if !buf.is_empty() => Ok(Err(StreamError::Closed)),
            Ok(n) => {
                buf.truncate(n);
                Ok(Ok(buf))
            }
            Err(FilesystemError::WouldBlock) => Ok(Ok(Vec::new())),
            Err(err) => self.push_stream_error(err).map(Err),
        }
    }

    /// Wait until `stream`'s poll finds `mask`, or an error or a hang-up.
    fn wait_on(&self, stream: &Stream, mask: PollMask) -> wasmtime::Result<()> {
        let pollable = table::Pollable::File(stream.file(), mask);
        self.wait_for(None, |table| pollable.ready(table).then_some(()))
    }
}

impl SocketsHost for Invocation {
    fn instance_network_instance_network(&mut self) -> wasmtime::Result<Network> {
        self.table().push(table::Network)
    }

    fn network_network_drop(&mut self, handle: Network) -> wasmtime::Result<()> {
        self.table().delete(handle).map(drop)
    }

    /// A lookup of `name`, answered through the stream it hands out. Without a
    /// network Grant no resolver answers, as WASI's `permanent-resolver-failure`
    /// says of a host without one.
    fn ip_name_lookup_resolve_addresses(
        &mut self,
        network: Network,
        name: String,
    ) -> wasmtime::Result<Result<ResolveAddressStream, ErrorCode>> {
        self.ensure_network(network)?;
        let Some(namespace) = self.wasi().table().network() else {
            return Ok(Err(ErrorCode::PermanentResolverFailure));
        };
        match namespace.resolve(&name) {
            Ok(lookup) => self
                .table()
                .push_file(table::ResolveAddressStream {
                    lookup,
                    handed: false,
                })
                .map(Ok),
            Err(err) => Ok(Err(to_lookup_error_code(err, true))),
        }
    }

    /// The name's one virtual address, then the end; a lookup still under way
    /// answers `would-block`.
    fn ip_name_lookup_resolve_address_stream_resolve_next_address(
        &mut self,
        this: ResolveAddressStream,
    ) -> wasmtime::Result<Result<Option<IpAddress>, ErrorCode>> {
        let stream = self.table().get(this)?;
        if stream.handed {
            return Ok(Ok(None));
        }
        let lookup = stream.lookup.clone();
        let real = match lookup.answer() {
            None => return Ok(Err(ErrorCode::WouldBlock)),
            Some(Err(err)) => return Ok(Err(to_lookup_error_code(err, false))),
            Some(Ok(real)) => real,
        };
        let namespace = self
            .wasi()
            .table()
            .network()
            .expect("a lookup is only made with a network");
        let Some(at) = namespace.alloc_virtual_address(lookup.name(), real) else {
            return Ok(Err(ErrorCode::TemporaryResolverFailure));
        };
        self.table().get_mut(this)?.handed = true;
        let [a, b, c, d] = at.octets();
        Ok(Ok(Some(IpAddress::Ipv4((a, b, c, d)))))
    }

    fn ip_name_lookup_resolve_address_stream_subscribe(
        &mut self,
        this: ResolveAddressStream,
    ) -> wasmtime::Result<Pollable> {
        let lookup = self.table().get(this)?.lookup.clone();
        let file = Arc::new(Mutex::new(
            lookup.alloc_file(self.wasi().descriptors().cred()),
        ));
        self.table().push(table::Pollable::File(file, PollMask::IN))
    }

    fn ip_name_lookup_resolve_address_stream_drop(
        &mut self,
        handle: ResolveAddressStream,
    ) -> wasmtime::Result<()> {
        self.table().delete(handle).map(drop)
    }

    /// Without a network Grant no socket is made, as WASI's `not-supported`
    /// says of a host without one; the guest is IPv4-only, so IPv6 is not
    /// supported either.
    fn tcp_create_socket_create_tcp_socket(
        &mut self,
        address_family: IpAddressFamily,
    ) -> wasmtime::Result<Result<TcpSocket, ErrorCode>> {
        if self.wasi().table().network().is_none() || address_family == IpAddressFamily::Ipv6 {
            return Ok(Err(ErrorCode::NotSupported));
        }
        if self.wasi().table().is_full() {
            return Ok(Err(ErrorCode::NewSocketLimit));
        }
        let sock = match self.network().create_tcp_socket() {
            Ok(sock) => sock,
            Err(err) => return Ok(Err(to_error_code(&err))),
        };
        let socket = SocketFile::new(sock.clone(), self.sock_alloc_file(sock));
        let handle: TcpSocket = self.table().push_file(table::TcpSocket {
            socket: socket.clone(),
            state: TcpState::Unbound,
        })?;
        self.wasi().table().install(handle, &socket);
        Ok(Ok(handle))
    }

    /// A connect passes the Invocation's namespace on what the guest named,
    /// which dials that exact address without blocking, so this answers at
    /// once.
    fn tcp_tcp_socket_start_connect(
        &mut self,
        this: TcpSocket,
        network: Network,
        remote_address: IpSocketAddress,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        self.ensure_network(network)?;
        let socket = self.tcp(this)?;
        if socket.state != TcpState::Unbound {
            return Ok(Err(ErrorCode::InvalidState));
        }
        let sock = socket.socket.sock.clone();
        let Some(addr) = to_socket_addr(&remote_address) else {
            return Ok(Err(ErrorCode::InvalidArgument));
        };
        let cred = self.wasi().descriptors().cred();
        if let Err(err) = self.network().connect(cred, &sock, addr) {
            return Ok(Err(to_error_code(&err)));
        }
        self.tcp(this)?.state = TcpState::Connecting;
        Ok(Ok(()))
    }

    /// The streams of a connect that landed, as WASI hands them out once; one
    /// still under way answers `would-block`, and a failed one closes the
    /// socket.
    fn tcp_tcp_socket_finish_connect(
        &mut self,
        this: TcpSocket,
    ) -> wasmtime::Result<Result<(InputStream, OutputStream), ErrorCode>> {
        let socket = self.tcp(this)?;
        if socket.state != TcpState::Connecting {
            return Ok(Err(ErrorCode::NotInProgress));
        }
        let socket = socket.socket.clone();
        match socket.sock.finish_connect() {
            Ok(()) => {
                self.tcp(this)?.state = TcpState::Connected;
                let stream = Stream::Socket(socket);
                let input = self.table().push(table::InputStream(stream.clone()))?;
                let output = self.table().push(table::OutputStream::new(stream))?;
                Ok(Ok((input, output)))
            }
            Err(FilesystemError::WouldBlock) => Ok(Err(ErrorCode::WouldBlock)),
            Err(err) => {
                self.tcp(this)?.state = TcpState::Closed;
                Ok(Err(to_error_code(&err)))
            }
        }
    }

    /// Ready once a connect settles, as the socket file polls writable; one
    /// connected to nothing polls hung up, so it is ready at once.
    fn tcp_tcp_socket_subscribe(&mut self, this: TcpSocket) -> wasmtime::Result<Pollable> {
        let socket = self.tcp(this)?.socket.clone();
        self.subscribe_stream(Stream::Socket(socket), PollMask::OUT)
    }

    fn tcp_tcp_socket_shutdown(
        &mut self,
        this: TcpSocket,
        shutdown_type: ShutdownType,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        let socket = self.tcp(this)?;
        if socket.state != TcpState::Connected {
            return Ok(Err(ErrorCode::InvalidState));
        }
        socket.socket.sock.shutdown(match shutdown_type {
            ShutdownType::Receive => Shutdown::Read,
            ShutdownType::Send => Shutdown::Write,
            ShutdownType::Both => Shutdown::Both,
        });
        Ok(Ok(()))
    }

    /// The peer as the guest named it.
    fn tcp_tcp_socket_remote_address(
        &mut self,
        this: TcpSocket,
    ) -> wasmtime::Result<Result<IpSocketAddress, ErrorCode>> {
        let socket = self.tcp(this)?;
        match (socket.state, socket.socket.sock.peer_addr()) {
            (TcpState::Connected, Some(peer)) => Ok(Ok(from_socket_addr(peer))),
            _ => Ok(Err(ErrorCode::InvalidState)),
        }
    }

    fn tcp_tcp_socket_drop(&mut self, handle: TcpSocket) -> wasmtime::Result<()> {
        self.table().delete(handle).map(drop)
    }

    /// Without a network Grant no socket is made, and the guest is IPv4-only,
    /// as a TCP socket's creation answers.
    fn udp_create_socket_create_udp_socket(
        &mut self,
        address_family: IpAddressFamily,
    ) -> wasmtime::Result<Result<UdpSocket, ErrorCode>> {
        if self.wasi().table().network().is_none() || address_family == IpAddressFamily::Ipv6 {
            return Ok(Err(ErrorCode::NotSupported));
        }
        if self.wasi().table().is_full() {
            return Ok(Err(ErrorCode::NewSocketLimit));
        }
        let sock = self.network().create_udp_socket();
        let socket = SocketFile::new(sock.clone(), self.sock_alloc_file(sock));
        let handle: UdpSocket = self.table().push_file(table::UdpSocket {
            socket: socket.clone(),
            state: UdpState::Unbound,
        })?;
        self.wasi().table().install(handle, &socket);
        Ok(Ok(handle))
    }

    /// The guest only sends to a peer it names, so the one bind it makes is
    /// the implicit one, to any address on an ephemeral port; any other is
    /// refused, as Landlock's `BIND_*` rights refuse one.
    fn udp_udp_socket_start_bind(
        &mut self,
        this: UdpSocket,
        network: Network,
        local_address: IpSocketAddress,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        self.ensure_network(network)?;
        if self.udp(this)?.state != UdpState::Unbound {
            return Ok(Err(ErrorCode::InvalidState));
        }
        let Some(addr) = to_socket_addr(&local_address) else {
            return Ok(Err(ErrorCode::InvalidArgument));
        };
        if let Err(err) = self.network().may_bind(addr) {
            return Ok(Err(to_error_code(&err)));
        }
        self.udp(this)?.state = UdpState::Binding;
        Ok(Ok(()))
    }

    fn udp_udp_socket_finish_bind(
        &mut self,
        this: UdpSocket,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        let socket = self.udp(this)?;
        if socket.state != UdpState::Binding {
            return Ok(Err(ErrorCode::NotInProgress));
        }
        socket.state = UdpState::Bound;
        Ok(Ok(()))
    }

    /// The streams to the one peer the guest names, which passes the
    /// Invocation's namespace as a connect does. A stream with no peer would
    /// send each datagram anywhere, so it is refused.
    fn udp_udp_socket_stream(
        &mut self,
        this: UdpSocket,
        remote_address: Option<IpSocketAddress>,
    ) -> wasmtime::Result<Result<(IncomingDatagramStream, OutgoingDatagramStream), ErrorCode>> {
        let socket = self.udp(this)?;
        if !matches!(socket.state, UdpState::Bound | UdpState::Streaming) {
            return Ok(Err(ErrorCode::InvalidState));
        }
        let datagrams = socket.socket.clone();
        let peer = match remote_address.as_ref().map(to_socket_addr) {
            Some(None) => return Ok(Err(ErrorCode::InvalidArgument)),
            peer => peer.flatten(),
        };
        let cred = self.wasi().descriptors().cred();
        if let Err(err) = self.network().connect_datagram(cred, &datagrams.sock, peer) {
            return Ok(Err(to_error_code(&err)));
        }
        self.udp(this)?.state = UdpState::Streaming;
        let incoming = self
            .table()
            .push(table::IncomingDatagramStream(datagrams.clone()))?;
        let outgoing = self.table().push(table::OutgoingDatagramStream {
            socket: datagrams,
            permit: 0,
        })?;
        Ok(Ok((incoming, outgoing)))
    }

    /// Ready at once, since a bind settles as it starts.
    fn udp_udp_socket_subscribe(&mut self, this: UdpSocket) -> wasmtime::Result<Pollable> {
        self.udp(this)?;
        self.table().push(table::Pollable::Ready)
    }

    fn udp_udp_socket_drop(&mut self, handle: UdpSocket) -> wasmtime::Result<()> {
        self.table().delete(handle).map(drop)
    }

    /// Up to `max_results` datagrams the peer sent, without waiting; none yet
    /// is an empty list.
    fn udp_incoming_datagram_stream_receive(
        &mut self,
        this: IncomingDatagramStream,
        max_results: u64,
    ) -> wasmtime::Result<Result<Vec<IncomingDatagram>, ErrorCode>> {
        let datagrams = self.table().get(this)?.0.clone();
        let peer = datagrams
            .sock
            .peer_addr()
            .expect("a datagram stream is only handed out once its socket connects");
        let mut received = Vec::new();
        let mut buf = vec![0; MAX_DATAGRAM];
        while (received.len() as u64) < max_results {
            match datagrams.file().read(&mut buf) {
                Ok(n) => received.push(IncomingDatagram {
                    data: buf[..n].to_vec(),
                    remote_address: from_socket_addr(peer),
                }),
                Err(FilesystemError::WouldBlock) => break,
                Err(err) if received.is_empty() => return Ok(Err(to_error_code(&err))),
                Err(_) => break,
            }
        }
        Ok(Ok(received))
    }

    fn udp_incoming_datagram_stream_subscribe(
        &mut self,
        this: IncomingDatagramStream,
    ) -> wasmtime::Result<Pollable> {
        let datagrams = self.table().get(this)?.0.clone();
        self.subscribe_datagrams(datagrams, PollMask::IN)
    }

    fn udp_incoming_datagram_stream_drop(
        &mut self,
        handle: IncomingDatagramStream,
    ) -> wasmtime::Result<()> {
        self.table().delete(handle).map(drop)
    }

    /// One datagram while the send queue has room, none while it is full.
    fn udp_outgoing_datagram_stream_check_send(
        &mut self,
        this: OutgoingDatagramStream,
    ) -> wasmtime::Result<Result<u64, ErrorCode>> {
        let datagrams = self.table().get(this)?.socket.clone();
        let found = datagrams.file().poll(None);
        let permitted = u64::from(found.contains(PollMask::OUT));
        *self.permit(this)? = permitted;
        Ok(Ok(permitted))
    }

    /// The datagrams sent in order, until the queue is full; one addressed to
    /// another peer, or past what UDP carries, is refused. More than the last
    /// `check-send` permitted traps, as `udp.wit` says it must, and a send
    /// spends the permit.
    fn udp_outgoing_datagram_stream_send(
        &mut self,
        this: OutgoingDatagramStream,
        datagrams: Vec<OutgoingDatagram>,
    ) -> wasmtime::Result<Result<u64, ErrorCode>> {
        let permit = std::mem::take(self.permit(this)?);
        if datagrams.len() as u64 > permit {
            return Err(wasmtime::Error::msg(
                "the guest sent more datagrams than check-send permitted",
            ));
        }
        let stream = self.table().get(this)?.socket.clone();
        let mut sent = 0;
        for datagram in datagrams {
            let addressed = datagram.remote_address.as_ref().map(to_socket_addr);
            let refused = if addressed.is_some_and(|addr| addr != stream.sock.peer_addr()) {
                Some(ErrorCode::InvalidArgument)
            } else if datagram.data.len() > MAX_DATAGRAM {
                Some(ErrorCode::DatagramTooLarge)
            } else {
                match stream.file().write(&datagram.data) {
                    Ok(_) => None,
                    Err(FilesystemError::WouldBlock) => break,
                    Err(err) => Some(to_error_code(&err)),
                }
            };
            match refused {
                None => sent += 1,
                Some(code) if sent == 0 => return Ok(Err(code)),
                Some(_) => break,
            }
        }
        Ok(Ok(sent))
    }

    fn udp_outgoing_datagram_stream_subscribe(
        &mut self,
        this: OutgoingDatagramStream,
    ) -> wasmtime::Result<Pollable> {
        let datagrams = self.table().get(this)?.socket.clone();
        self.subscribe_datagrams(datagrams, PollMask::OUT)
    }

    fn udp_outgoing_datagram_stream_drop(
        &mut self,
        handle: OutgoingDatagramStream,
    ) -> wasmtime::Result<()> {
        self.table().delete(handle).map(drop)
    }
}

impl IoHost for Invocation {
    fn streams_input_stream_blocking_read(
        &mut self,
        this: InputStream,
        len: u64,
    ) -> wasmtime::Result<Result<Vec<u8>, StreamError>> {
        let stream = self.table().get(this)?.0.clone();
        self.wait_on(&stream, PollMask::IN)?;
        self.read_now(&stream, len)
    }

    /// What the stream holds now, none while it holds nothing yet.
    fn streams_input_stream_read(
        &mut self,
        this: InputStream,
        len: u64,
    ) -> wasmtime::Result<Result<Vec<u8>, StreamError>> {
        let stream = self.table().get(this)?.0.clone();
        self.read_now(&stream, len)
    }

    /// A pollable the stream must outlive, as `io.wit` makes it a child.
    fn streams_input_stream_subscribe(&mut self, this: InputStream) -> wasmtime::Result<Pollable> {
        let stream = self.table().get(this)?.0.clone();
        self.table()
            .push_child(table::Pollable::File(stream.file(), PollMask::IN), this)
    }

    fn streams_input_stream_drop(&mut self, handle: InputStream) -> wasmtime::Result<()> {
        self.table().delete(handle).map(drop)
    }

    /// Write every byte, at most 4096, waiting for room as it goes, then until
    /// what was written has left for the host.
    fn streams_output_stream_blocking_write_and_flush(
        &mut self,
        this: OutputStream,
        contents: Vec<u8>,
    ) -> wasmtime::Result<Result<(), StreamError>> {
        if contents.len() > MAX_BLOCKING_WRITE {
            return Err(wasmtime::Error::msg(
                "the guest handed blocking-write-and-flush more than 4096 bytes",
            ));
        }
        let stream = self.table().get(this)?.stream.clone();
        let mut rest = &contents[..];
        while !rest.is_empty() {
            self.wait_on(&stream, PollMask::OUT)?;
            let written = stream.write(rest);
            match written {
                Ok(n) => rest = &rest[n..],
                Err(FilesystemError::WouldBlock) => {}
                Err(err) => return self.push_stream_error(err).map(Err),
            }
        }
        // Registering through the stream's poll puts the wait on its queue,
        // which a socket's writer wakes after each chunk it sends.
        self.wait_for(None, |table| {
            let failed = stream.poll(table).contains(PollMask::ERR);
            (failed || stream.is_drained()).then_some(())
        })?;
        let written = stream.write(&[]);
        match written {
            Ok(_) => Ok(Ok(())),
            Err(err) => self.push_stream_error(err).map(Err),
        }
    }

    /// How many bytes a write takes now, zero while the stream is full or a
    /// flush is under way; a failed stream answers its failure.
    fn streams_output_stream_check_write(
        &mut self,
        this: OutputStream,
    ) -> wasmtime::Result<Result<u64, StreamError>> {
        let stream = self.table().get(this)?.stream.clone();
        if let Err(err) = stream.write(&[]) {
            return self.push_stream_error(err).map(Err);
        }
        let output = self.table().get_mut(this)?;
        if output.flushing && !stream.is_drained() {
            return Ok(Ok(0));
        }
        output.flushing = false;
        Ok(Ok(stream.room() as u64))
    }

    /// Write what `check-write` permitted; more than it permits traps, as
    /// `io.wit` says a write past the permit must.
    fn streams_output_stream_write(
        &mut self,
        this: OutputStream,
        contents: Vec<u8>,
    ) -> wasmtime::Result<Result<(), StreamError>> {
        let stream = self.table().get(this)?.stream.clone();
        if contents.len() > stream.room() {
            return Err(wasmtime::Error::msg(
                "the guest wrote past what check-write permitted",
            ));
        }
        match stream.write(&contents) {
            Ok(_) => Ok(Ok(())),
            Err(err) => self.push_stream_error(err).map(Err),
        }
    }

    /// Start a flush, which completes once what was written has left for the
    /// host; `check-write` answers zero until then, as `io.wit` asks.
    fn streams_output_stream_flush(
        &mut self,
        this: OutputStream,
    ) -> wasmtime::Result<Result<(), StreamError>> {
        let stream = self.table().get(this)?.stream.clone();
        if let Err(err) = stream.write(&[]) {
            return self.push_stream_error(err).map(Err);
        }
        self.table().get_mut(this)?.flushing = true;
        Ok(Ok(()))
    }

    /// A pollable ready once the stream takes a write, the stream its parent,
    /// as `io.wit` makes it a child.
    fn streams_output_stream_subscribe(
        &mut self,
        this: OutputStream,
    ) -> wasmtime::Result<Pollable> {
        let stream = self.table().get(this)?.stream.clone();
        self.table()
            .push_child(table::Pollable::File(stream.file(), PollMask::OUT), this)
    }

    fn streams_output_stream_drop(&mut self, handle: OutputStream) -> wasmtime::Result<()> {
        self.table().delete(handle).map(drop)
    }

    /// The failure's POSIX name, so no host text reaches the guest.
    fn error_error_to_debug_string(&mut self, this: Error) -> wasmtime::Result<String> {
        let err = &self.table().get(this)?.0;
        Ok(err.posix_name().unwrap_or("EIO").to_string())
    }

    fn error_error_drop(&mut self, handle: Error) -> wasmtime::Result<()> {
        self.table().delete(handle).map(drop)
    }

    fn poll_pollable_block(&mut self, this: Pollable) -> wasmtime::Result<()> {
        let pollable = self.table().get(this)?.clone();
        let producer = pollable.is_producer();
        let pollable = pollable.for_wait(self.profile(), producer);
        self.wait_for(pollable.due(), |table| pollable.ready(table).then_some(()))
    }

    /// The indices of the pollables ready, once one is, as WASI's `poll`
    /// answers; an empty list traps, as WASI says it does.
    fn poll_poll(&mut self, r#in: Vec<Pollable>) -> wasmtime::Result<Vec<u32>> {
        if r#in.is_empty() {
            return Err(wasmtime::Error::msg("poll was handed no pollable"));
        }
        let pollables = r#in
            .into_iter()
            .map(|handle| self.table().get(handle).cloned())
            .collect::<wasmtime::Result<Vec<_>>>()?;
        let producer = pollables.iter().any(table::Pollable::is_producer);
        let profile = self.profile();
        let pollables = pollables
            .into_iter()
            .map(|pollable| pollable.for_wait(profile, producer))
            .collect::<Vec<_>>();
        let due = pollables.iter().filter_map(table::Pollable::due).min();
        self.wait_for(due, |mut table| {
            let ready = (0..pollables.len() as u32)
                .filter(|i| pollables[*i as usize].ready(table.as_deref_mut()))
                .collect::<Vec<_>>();
            (!ready.is_empty()).then_some(ready)
        })
    }

    fn poll_pollable_drop(&mut self, handle: Pollable) -> wasmtime::Result<()> {
        self.table().delete(handle).map(drop)
    }
}

impl ClocksHost for Invocation {
    fn monotonic_clock_subscribe_duration(
        &mut self,
        when: monotonic_clock::Duration,
    ) -> wasmtime::Result<Pollable> {
        self.subscribe_clock(when)
    }
}

#[cfg(test)]
mod tests {
    //! Witness each preview2 import through a core module that links it by its
    //! WASI name, as a Guest Binary does.
    use std::time::{Duration, Instant};

    use wasmtime::{Instance, Module, Store};

    use crate::cache::load_engine;
    use crate::config::Config;
    use crate::imports;
    use crate::invocation::Invocation;
    use crate::trap::NO_TIMEOUT_EPOCH_DELTA;
    use std::sync::Arc;

    use super::super::bindings::wasi_sockets_network_0_2_12::Ipv4SocketAddress;
    use super::*;
    use crate::wasi::{sandbox_connections, Wasi};
    use kobako_runtime::grant::Grant;
    use kobako_runtime::network;
    use kobako_runtime::profile::Profile;
    use kobako_runtime::sched::run_task;

    /// Imports one function of each preview2 interface kobako answers, with a
    /// bump `cabi_realloc` from 4096.
    const GUEST_WAT: &str = r#"
        (module
          (import "wasi:sockets/tcp-create-socket@0.2.12" "create-tcp-socket"
            (func $create (param i32 i32)))
          (import "wasi:sockets/instance-network@0.2.12" "instance-network"
            (func $network (result i32)))
          (import "wasi:sockets/network@0.2.12" "[resource-drop]network"
            (func $drop_network (param i32)))
          (import "wasi:clocks/monotonic-clock@0.2.12" "subscribe-duration"
            (func $subscribe (param i64) (result i32)))
          (import "wasi:io/poll@0.2.12" "[method]pollable.block" (func $block (param i32)))
          (import "wasi:io/poll@0.2.12" "poll" (func $poll (param i32 i32 i32)))
          (import "wasi:sockets/ip-name-lookup@0.2.12" "resolve-addresses"
            (func $resolve (param i32 i32 i32 i32)))
          (memory (export "memory") 1)
          (global $bump (mut i32) (i32.const 4096))
          (func (export "cabi_realloc") (param i32 i32 i32 i32) (result i32)
            (global.get $bump)
            (global.set $bump (i32.add (global.get $bump) (local.get 3))))
          (func (export "create_ipv4") (call $create (i32.const 0) (i32.const 16)))
          (func (export "network") (result i32) (call $network))
          (func (export "drop_network") (param i32) (call $drop_network (local.get 0)))
          (func (export "subscribe") (param i64) (result i32) (call $subscribe (local.get 0)))
          (func (export "block") (param i32) (call $block (local.get 0)))
          (func (export "poll") (param i32 i32)
            (i32.store (i32.const 64) (local.get 0))
            (call $poll (i32.const 64) (local.get 1) (i32.const 32)))
          (func (export "resolve") (param i32 i32)
            (call $resolve (local.get 0) (i32.const 128) (local.get 1) (i32.const 32))))
    "#;

    /// A guest at `profile`, holding at most `open_file_limit` files.
    fn guest(profile: Profile, open_file_limit: Option<usize>) -> (Store<Invocation>, Instance) {
        let engine = load_engine().expect("shared engine must be constructible");
        let config = Config {
            timeout: None,
            setup_timeout: None,
            memory_limit: None,
            stdout_limit: None,
            stderr_limit: None,
            open_file_limit,
            open_connection_limit: None,
            connection_limit: None,
            profile,
            security: Vec::new(),
        };
        let wasi = Wasi::for_invocation(&config, Vec::new(), &[], &sandbox_connections())
            .expect("WASI context must build");
        let mut store = Store::new(engine, Invocation::new(None, wasi));
        store.set_epoch_deadline(NO_TIMEOUT_EPOCH_DELTA);
        let module = Module::new(engine, GUEST_WAT).expect("the guest module must compile");
        let instance = imports::linker()
            .expect("host imports must link")
            .instantiate(&mut store, &module)
            .expect("every preview2 import the module names must link");
        (store, instance)
    }

    fn call<P: wasmtime::WasmParams, R: wasmtime::WasmResults>(
        store: &mut Store<Invocation>,
        instance: &Instance,
        name: &str,
        params: P,
    ) -> wasmtime::Result<R> {
        instance
            .get_typed_func::<P, R>(&mut *store, name)
            .expect("the guest module exports each call")
            .call(&mut *store, params)
    }

    fn read(store: &mut Store<Invocation>, instance: &Instance, at: usize, len: usize) -> Vec<u8> {
        let memory = instance.get_memory(&mut *store, "memory").expect("memory");
        memory.data(&*store)[at..at + len].to_vec()
    }

    // @behavior NW-009
    #[test]
    fn without_the_network_creating_a_socket_answers_not_supported() {
        let (mut store, instance) = guest(Profile::Hermetic, None);
        call::<(), ()>(&mut store, &instance, "create_ipv4", ()).expect("the call answers");
        let answer = read(&mut store, &instance, 16, 5);
        assert_eq!(
            (answer[0], answer[4]),
            (1, 2),
            "create-tcp-socket with no network entry must answer err(not-supported)"
        );
    }

    // @behavior NW-010
    #[test]
    fn the_network_is_one_handle_the_guest_may_drop() {
        let (mut store, instance) = guest(Profile::Hermetic, None);
        let network = call::<(), i32>(&mut store, &instance, "network", ()).expect("a handle");
        let first = call::<i32, ()>(&mut store, &instance, "drop_network", network);
        let second = call::<i32, ()>(&mut store, &instance, "drop_network", network);
        assert!(
            first.is_ok() && second.is_err(),
            "[resource-drop]network of the handle instance-network answered must succeed once and \
             then trap"
        );
    }

    /// A host that has given every invocation up.
    struct GivenUp;

    impl kobako_runtime::DispatchHandler for GivenUp {
        fn dispatch(
            &self,
            _call: kobako_transport::envelope::Call<'_>,
            _yielder: &mut dyn kobako_runtime::yielder::Yielder,
        ) -> Option<kobako_transport::envelope::Reply> {
            None
        }

        fn abandoned(&self) -> bool {
            true
        }
    }

    // @behavior RT-354
    #[test]
    fn a_preview2_call_answered_after_the_host_gave_up_traps() {
        let (mut store, instance) = guest(Profile::Hermetic, None);
        store.data_mut().bind_on_dispatch(Arc::new(GivenUp));
        let network = call::<(), i32>(&mut store, &instance, "network", ());
        let created = call::<(), ()>(&mut store, &instance, "create_ipv4", ());
        assert_eq!(
            (
                network.map_err(|trap| trap.root_cause().to_string()),
                created.map_err(|trap| trap.root_cause().to_string())
            ),
            (
                Err(kobako_runtime::endpoint::ABANDONED.to_string()),
                Err(kobako_runtime::endpoint::ABANDONED.to_string())
            ),
            "instance-network and create-tcp-socket answered after the host abandoned the \
             invocation must each trap as abandoned"
        );
    }

    // @behavior RT-353
    #[test]
    fn a_list_past_the_guests_memory_traps_before_the_host_reads_it() {
        let (mut store, instance) = guest(Profile::Hermetic, None);
        let network = call::<(), i32>(&mut store, &instance, "network", ()).expect("a handle");
        for len in [-1, 1 << 20] {
            let polled = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                call::<(i32, i32), ()>(&mut store, &instance, "poll", (0, len))
            }));
            let resolved = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                call::<(i32, i32), ()>(&mut store, &instance, "resolve", (network, len))
            }));
            assert!(
                matches!((polled, resolved), (Ok(Err(_)), Ok(Err(_)))),
                "wasi:io/poll.poll naming {len} pollables, and resolve-addresses naming a {len}-byte \
                 name, each past one page of memory, must trap rather than panic or answer"
            );
        }
    }

    // @behavior RT-338
    #[test]
    fn a_clock_wait_on_the_strongest_posture_ends_at_once() {
        let (mut store, instance) = guest(Profile::Hermetic, None);
        let started = Instant::now();
        let pollable = call::<i64, i32>(&mut store, &instance, "subscribe", 10_000_000_000)
            .expect("a pollable");
        call::<i32, ()>(&mut store, &instance, "block", pollable).expect("the wait answers");
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "pollable.block on ten seconds of subscribe-duration at the hermetic posture must \
             return at once, not after {:?}",
            started.elapsed()
        );
    }

    // @behavior RT-339
    #[test]
    fn a_poll_on_a_clock_waits_until_it_is_due() {
        let (mut store, instance) = guest(Profile::Permissive, None);
        let started = Instant::now();
        let pollable =
            call::<i64, i32>(&mut store, &instance, "subscribe", 50_000_000).expect("a pollable");
        run_task(Some(started + Duration::from_secs(10)), || {
            call::<(i32, i32), ()>(&mut store, &instance, "poll", (pollable, 1))
        })
        .expect("the poll answers");
        let spent = started.elapsed();
        let answer = read(&mut store, &instance, 32, 8);
        let (at, len) = (
            u32::from_le_bytes(answer[..4].try_into().unwrap()) as usize,
            u32::from_le_bytes(answer[4..].try_into().unwrap()),
        );
        let index = u32::from_le_bytes(read(&mut store, &instance, at, 4).try_into().unwrap());
        assert!(
            (len, index) == (1, 0)
                && spent >= Duration::from_millis(50)
                && spent < Duration::from_secs(5),
            "poll of one pollable on fifty milliseconds of subscribe-duration at the permissive \
             posture must answer [0] once they pass, got {len} index {index} after {spent:?}"
        );
    }

    /// A hermetic invocation whose network reaches `port` on loopback over TCP.
    fn networked(port: u16) -> Invocation {
        within_network(
            network::Endpoint::new("127.0.0.1", port)
                .unwrap()
                .with_protocol(network::Protocol::Tcp),
        )
    }

    /// A hermetic invocation whose network holds `endpoint`, or none.
    fn within_network(endpoint: network::Endpoint) -> Invocation {
        within_network_holding(endpoint, None)
    }

    /// A hermetic invocation whose network holds `endpoint`, holding at most
    /// `open_file_limit` files.
    fn within_network_holding(
        endpoint: network::Endpoint,
        open_file_limit: Option<usize>,
    ) -> Invocation {
        let config = Config {
            timeout: None,
            setup_timeout: None,
            memory_limit: None,
            stdout_limit: None,
            stderr_limit: None,
            open_file_limit,
            open_connection_limit: None,
            connection_limit: None,
            profile: Profile::Hermetic,
            security: Vec::new(),
        };
        let mut entry = network::Builder::default();
        entry.endpoint(endpoint);
        let grant = Grant::Network(network::Network::new("local", Arc::new(entry)).unwrap());
        let wasi = Wasi::for_invocation(&config, Vec::new(), &[grant], &sandbox_connections())
            .expect("WASI context must build");
        Invocation::new(None, wasi)
    }

    // @behavior NW-030
    #[test]
    fn without_the_network_a_lookup_answers_permanent_resolver_failure() {
        let (mut store, _instance) = guest(Profile::Hermetic, None);
        let guest = store.data_mut();
        let network = guest.instance_network_instance_network().unwrap();
        assert!(
            matches!(
                guest.ip_name_lookup_resolve_addresses(network, "example.com".to_string()),
                Ok(Err(ErrorCode::PermanentResolverFailure))
            ),
            "resolve-addresses with no network entry must answer permanent-resolver-failure"
        );
    }

    // @behavior NW-031
    #[test]
    fn a_looked_up_name_connects_and_reads_back_its_virtual_address() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut guest = within_network(
            network::Endpoint::new("localhost", port)
                .unwrap()
                .with_protocol(network::Protocol::Tcp)
                .with_addresses(vec![network::Addresses::Local]),
        );
        let started = Instant::now();
        let (handed, end, remote) = run_task(Some(started + Duration::from_secs(10)), || {
            let network = guest.instance_network_instance_network()?;
            let stream = guest
                .ip_name_lookup_resolve_addresses(network, "localhost".to_string())?
                .expect("an endpoint names the name");
            let answered = guest.ip_name_lookup_resolve_address_stream_subscribe(stream)?;
            guest.poll_pollable_block(answered)?;
            let handed = guest
                .ip_name_lookup_resolve_address_stream_resolve_next_address(stream)?
                .expect("localhost resolves");
            let end = guest.ip_name_lookup_resolve_address_stream_resolve_next_address(stream)?;
            let Some(IpAddress::Ipv4(address)) = handed else {
                return Err(wasmtime::Error::msg("no IPv4 address handed out"));
            };
            let socket = guest
                .tcp_create_socket_create_tcp_socket(IpAddressFamily::Ipv4)?
                .expect("a network makes a socket");
            let remote = IpSocketAddress::Ipv4(Ipv4SocketAddress { port, address });
            guest
                .tcp_tcp_socket_start_connect(socket, network, remote)?
                .expect("the endpoint leads to loopback");
            let connecting = guest.tcp_tcp_socket_subscribe(socket)?;
            guest.poll_pollable_block(connecting)?;
            guest
                .tcp_tcp_socket_finish_connect(socket)?
                .expect("the listener accepts");
            Ok((handed, end, guest.tcp_tcp_socket_remote_address(socket)?))
        })
        .expect("the lookup and the connect answer");
        assert!(
            matches!(handed, Some(IpAddress::Ipv4((198, 18, 0, 0))))
                && matches!(end, Ok(None))
                && matches!(
                    remote,
                    Ok(IpSocketAddress::Ipv4(Ipv4SocketAddress {
                        address: (198, 18, 0, 0),
                        ..
                    }))
                ),
            "a lookup of a name an endpoint names must hand out one virtual address then end, and \
             a socket connected through it must read that virtual address back as its peer"
        );
    }

    // @behavior NW-055
    #[test]
    fn a_udp_socket_streams_only_with_a_peer_it_names() {
        let peer = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let port = peer.local_addr().unwrap().port();
        let mut guest = within_network(
            network::Endpoint::new("127.0.0.1", port)
                .unwrap()
                .with_protocol(network::Protocol::Udp),
        );
        let at = |port| {
            IpSocketAddress::Ipv4(Ipv4SocketAddress {
                port,
                address: (127, 0, 0, 1),
            })
        };
        let any = |port| {
            IpSocketAddress::Ipv4(Ipv4SocketAddress {
                port,
                address: (0, 0, 0, 0),
            })
        };
        let started = Instant::now();
        let (fixed, peerless, answer) = run_task(Some(started + Duration::from_secs(10)), || {
            let network = guest.instance_network_instance_network()?;
            let fixed_socket = guest
                .udp_create_socket_create_udp_socket(IpAddressFamily::Ipv4)?
                .expect("a network makes a socket");
            let fixed = guest.udp_udp_socket_start_bind(fixed_socket, network, any(4000))?;
            let socket = guest
                .udp_create_socket_create_udp_socket(IpAddressFamily::Ipv4)?
                .expect("a network makes a socket");
            guest
                .udp_udp_socket_start_bind(socket, network, any(0))?
                .expect("an ephemeral bind is the implicit one");
            guest
                .udp_udp_socket_finish_bind(socket)?
                .expect("the bind settles");
            let peerless = guest.udp_udp_socket_stream(socket, None)?.map(drop);
            let (incoming, outgoing) = guest
                .udp_udp_socket_stream(socket, Some(at(port)))?
                .expect("the udp endpoint names the peer");
            let room = guest.udp_outgoing_datagram_stream_subscribe(outgoing)?;
            guest.poll_pollable_block(room)?;
            guest
                .udp_outgoing_datagram_stream_check_send(outgoing)?
                .expect("the stream with room permits a datagram");
            let sent = guest.udp_outgoing_datagram_stream_send(
                outgoing,
                vec![OutgoingDatagram {
                    data: b"ping".to_vec(),
                    remote_address: None,
                }],
            )?;
            if !matches!(sent, Ok(1)) {
                return Err(wasmtime::Error::msg("the datagram was not sent"));
            }
            let mut buf = [0; 16];
            let (n, from) = peer.recv_from(&mut buf)?;
            peer.send_to(&buf[..n].to_ascii_uppercase(), from)?;
            let arrived = guest.udp_incoming_datagram_stream_subscribe(incoming)?;
            guest.poll_pollable_block(arrived)?;
            let answer = guest
                .udp_incoming_datagram_stream_receive(incoming, 4)?
                .expect("the peer's datagram is received");
            Ok((fixed, peerless, answer))
        })
        .expect("the bind, the stream, and the round trip answer");
        assert!(
            matches!(fixed, Err(ErrorCode::AccessDenied))
                && matches!(peerless, Err(ErrorCode::AccessDenied))
                && answer.len() == 1
                && answer[0].data == b"PING"
                && matches!(
                    answer[0].remote_address,
                    IpSocketAddress::Ipv4(Ipv4SocketAddress {
                        address: (127, 0, 0, 1),
                        port: p,
                    }) if p == port
                ),
            "a UDP socket must refuse a fixed-port bind and a stream with no peer as \
             access-denied, and carry a datagram both ways with the peer a udp endpoint names; \
             got bind {fixed:?}, peerless {peerless:?}, received {} datagram(s)",
            answer.len()
        );
    }

    // @behavior NW-072
    #[test]
    fn a_datagram_send_past_its_permit_traps() {
        let peer = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let port = peer.local_addr().unwrap().port();
        let mut guest = within_network(
            network::Endpoint::new("127.0.0.1", port)
                .unwrap()
                .with_protocol(network::Protocol::Udp),
        );
        let ping = || {
            vec![OutgoingDatagram {
                data: b"ping".to_vec(),
                remote_address: None,
            }]
        };
        let (unpermitted, permit, sent, spent) = run_task(
            Some(Instant::now() + Duration::from_secs(10)),
            || -> wasmtime::Result<_> {
                let network = guest.instance_network_instance_network()?;
                let socket = guest
                    .udp_create_socket_create_udp_socket(IpAddressFamily::Ipv4)?
                    .expect("a network makes a socket");
                guest
                    .udp_udp_socket_start_bind(
                        socket,
                        network,
                        IpSocketAddress::Ipv4(Ipv4SocketAddress {
                            port: 0,
                            address: (0, 0, 0, 0),
                        }),
                    )?
                    .expect("an ephemeral bind is the implicit one");
                guest
                    .udp_udp_socket_finish_bind(socket)?
                    .expect("the bind settles");
                let (_incoming, outgoing) = guest
                    .udp_udp_socket_stream(
                        socket,
                        Some(IpSocketAddress::Ipv4(Ipv4SocketAddress {
                            port,
                            address: (127, 0, 0, 1),
                        })),
                    )?
                    .expect("the udp endpoint names the peer");
                let room = guest.udp_outgoing_datagram_stream_subscribe(outgoing)?;
                guest.poll_pollable_block(room)?;
                let unpermitted = guest
                    .udp_outgoing_datagram_stream_send(outgoing, ping())
                    .is_err();
                let permit = guest.udp_outgoing_datagram_stream_check_send(outgoing)?;
                let sent = guest.udp_outgoing_datagram_stream_send(outgoing, ping())?;
                let spent = guest
                    .udp_outgoing_datagram_stream_send(outgoing, ping())
                    .is_err();
                wasmtime::Result::Ok((unpermitted, permit, sent, spent))
            },
        )
        .expect("the socket streams with its peer");
        assert_eq!(
            (unpermitted, permit, sent, spent),
            (true, Ok(1), Ok(1), true),
            "outgoing-datagram-stream.send must trap before any check-send and once its permit \
             is spent, and send what check-send permitted"
        );
    }

    // @behavior NW-073
    #[test]
    fn a_blocking_write_past_4096_bytes_traps() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let mut guest = networked(listener.local_addr().unwrap().port());
        let answers = run_task(
            Some(Instant::now() + Duration::from_secs(10)),
            || -> wasmtime::Result<_> {
                let socket = guest
                    .tcp_create_socket_create_tcp_socket(IpAddressFamily::Ipv4)?
                    .expect("a network makes a socket");
                let network = guest.instance_network_instance_network()?;
                let remote = IpSocketAddress::Ipv4(Ipv4SocketAddress {
                    port: listener.local_addr().unwrap().port(),
                    address: (127, 0, 0, 1),
                });
                guest
                    .tcp_tcp_socket_start_connect(socket, network, remote)?
                    .expect("an endpoint names the address");
                let connecting = guest.tcp_tcp_socket_subscribe(socket)?;
                guest.poll_pollable_block(connecting)?;
                let (_input, output) = guest
                    .tcp_tcp_socket_finish_connect(socket)?
                    .expect("the listener accepts");
                let past = guest
                    .streams_output_stream_blocking_write_and_flush(output, vec![0; 4097])
                    .is_err();
                let within = guest
                    .streams_output_stream_blocking_write_and_flush(output, vec![0; 4096])
                    .map(|answer| answer.is_ok())?;
                wasmtime::Result::Ok((past, within))
            },
        )
        .expect("the socket connects");
        assert_eq!(
            answers,
            (true, true),
            "output-stream.blocking-write-and-flush must trap on 4097 bytes, as io.wit bounds it, \
             and write 4096"
        );
    }

    // @behavior NW-023
    #[test]
    fn a_frozen_clock_beside_a_socket_elapses_in_host_time() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let mut guest = networked(listener.local_addr().unwrap().port());
        let started = Instant::now();
        let answer = run_task(Some(started + Duration::from_secs(10)), || {
            let socket = guest
                .tcp_create_socket_create_tcp_socket(IpAddressFamily::Ipv4)?
                .expect("a network makes a socket");
            let network = guest.instance_network_instance_network()?;
            let remote = IpSocketAddress::Ipv4(Ipv4SocketAddress {
                port: listener.local_addr().unwrap().port(),
                address: (127, 0, 0, 1),
            });
            guest
                .tcp_tcp_socket_start_connect(socket, network, remote)?
                .expect("an endpoint names the address");
            let connecting = guest.tcp_tcp_socket_subscribe(socket)?;
            guest.poll_pollable_block(connecting)?;
            let (input, _output) = guest
                .tcp_tcp_socket_finish_connect(socket)?
                .expect("the listener accepts");
            let silent = guest.streams_input_stream_subscribe(input)?;
            let clock = guest.monotonic_clock_subscribe_duration(50_000_000)?;
            guest.poll_poll(vec![silent, clock])
        })
        .expect("the poll answers");
        let spent = started.elapsed();
        assert!(
            answer == vec![1] && spent >= Duration::from_millis(50),
            "poll of a silent socket beside fifty milliseconds of subscribe-duration at the \
             hermetic posture must answer the clock once they pass, got {answer:?} after \
             {spent:?}"
        );
    }

    // @behavior RT-340
    #[test]
    fn a_poll_of_no_pollable_traps() {
        let (mut store, instance) = guest(Profile::Hermetic, None);
        assert!(
            call::<(i32, i32), ()>(&mut store, &instance, "poll", (0, 0)).is_err(),
            "poll of an empty list must trap"
        );
    }

    // @behavior RT-341
    #[test]
    fn a_view_takes_no_file_of_the_open_file_limit() {
        let (mut store, instance) = guest(Profile::Hermetic, Some(3));
        let first = call::<i64, i32>(&mut store, &instance, "subscribe", 0);
        let guest = store.data_mut();
        let held = (1..crate::descriptors::MAX_NUMBERS - 3)
            .map(|_| guest.monotonic_clock_subscribe_duration(0))
            .take_while(Result::is_ok)
            .count();
        let past = guest.monotonic_clock_subscribe_duration(0);
        assert_eq!(
            (first.is_ok(), held, past.is_err()),
            (true, crate::descriptors::MAX_NUMBERS - 4, true),
            "subscribe-duration with the open file limit filled by the standard streams must \
             hand out pollables up to a million handles, then trap"
        );
    }

    // @behavior NW-069
    #[test]
    fn a_socket_past_the_open_file_limit_answers_new_socket_limit() {
        let endpoint = network::Endpoint::new("127.0.0.1", 9)
            .unwrap()
            .with_protocol(network::Protocol::Tcp);
        let mut guest = within_network_holding(endpoint, Some(3));
        assert_eq!(
            (
                guest
                    .tcp_create_socket_create_tcp_socket(IpAddressFamily::Ipv4)
                    .ok()
                    .and_then(|answer| answer.err()),
                guest
                    .udp_create_socket_create_udp_socket(IpAddressFamily::Ipv4)
                    .ok()
                    .and_then(|answer| answer.err()),
            ),
            (
                Some(ErrorCode::NewSocketLimit),
                Some(ErrorCode::NewSocketLimit)
            ),
            "create-tcp-socket and create-udp-socket with the open file limit filled by the \
             standard streams must answer new-socket-limit, not trap"
        );
    }

    // @behavior NW-069
    #[test]
    fn a_socket_with_one_file_left_connects_and_hands_out_its_streams() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let endpoint = network::Endpoint::new("127.0.0.1", port)
            .unwrap()
            .with_protocol(network::Protocol::Tcp)
            .with_addresses(vec![network::Addresses::Local]);
        let mut guest = within_network_holding(endpoint, Some(4));
        let started = Instant::now();
        let connected = run_task(Some(started + Duration::from_secs(10)), || {
            let network = guest.instance_network_instance_network()?;
            let socket = guest
                .tcp_create_socket_create_tcp_socket(IpAddressFamily::Ipv4)?
                .expect("one file is left for the socket");
            let remote = IpSocketAddress::Ipv4(Ipv4SocketAddress {
                port,
                address: (127, 0, 0, 1),
            });
            guest
                .tcp_tcp_socket_start_connect(socket, network, remote)?
                .expect("the endpoint names the listener");
            let connecting = guest.tcp_tcp_socket_subscribe(socket)?;
            guest.poll_pollable_block(connecting)?;
            Ok::<_, wasmtime::Error>(guest.tcp_tcp_socket_finish_connect(socket)?.is_ok())
        });
        assert!(
            matches!(connected, Ok(true)),
            "a TCP socket made with one file left under the open file limit must connect and \
             hand out its streams, as Linux's connect takes no fd, not {connected:?}"
        );
    }

    /// The preview2 functions a scenario answers before a Guest Binary
    /// imports them, each beside the scenario that holds the answer.
    const ANSWERED_AHEAD: [(&str, &str, &str); 1] = [(
        "wasi:sockets/tcp@0.2.12",
        "[method]tcp-socket.remote-address",
        "NW-031",
    )];

    #[test]
    fn the_driver_links_only_the_preview2_functions_a_guest_or_a_scenario_calls() {
        let data = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../data"));
        let binaries = std::fs::read_dir(data)
            .map(|dir| {
                dir.filter_map(Result::ok)
                    .map(|entry| entry.path())
                    .filter(|path| path.extension().is_some_and(|ext| ext == "wasm"))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if binaries.is_empty() {
            assert!(
                std::env::var_os("CI").is_none(),
                "data/*.wasm missing under CI — run `bundle exec rake wasm:build`"
            );
            return;
        }
        let preview2 = |module: &str| module.starts_with("wasi:") && module.contains("@0.2");
        let mut imported = std::collections::BTreeSet::new();
        for binary in &binaries {
            let module = crate::cache::load_module(binary).expect("a Guest Binary must load");
            for import in module.imports().filter(|import| preview2(import.module())) {
                imported.insert((import.module().to_string(), import.name().to_string()));
            }
        }
        let (mut store, _) = guest(Profile::Hermetic, None);
        let linker = imports::linker().expect("host imports must link");
        let linked = linker
            .iter(&mut store)
            .filter(|(module, _, _)| preview2(module))
            .map(|(module, name, _)| (module.to_string(), name.to_string()))
            .collect::<std::collections::BTreeSet<_>>();
        let ahead = ANSWERED_AHEAD
            .iter()
            .map(|(module, name, _)| (module.to_string(), name.to_string()))
            .collect::<std::collections::BTreeSet<_>>();
        let expected = imported
            .union(&ahead)
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            linked, expected,
            "the Wasm Driver must link exactly the preview2 functions some data/*.wasm imports \
             and those a scenario answers ahead of the guest"
        );
    }

    // @behavior RT-347
    #[test]
    fn a_handle_and_a_descriptor_number_from_one_space() {
        let (mut store, _) = guest(Profile::Hermetic, None);
        let guest = store.data_mut();
        let first = guest.instance_network_instance_network().unwrap();
        let next_descriptor = guest.wasi().descriptors().lowest_unused();
        guest.network_network_drop(first).unwrap();
        let again = guest.instance_network_instance_network().unwrap();
        assert_eq!(
            (first.0, next_descriptor, again.0),
            (3, Some(4), 3),
            "a guest holding only stdin, stdout, and stderr must take 3 for its first preview2 \
             handle, find 4 as preview1's next descriptor, and take 3 again once it is freed"
        );
    }

    // @behavior RT-348
    #[test]
    fn a_drop_of_a_handle_as_another_kind_traps() {
        let (mut store, _) = guest(Profile::Hermetic, None);
        let guest = store.data_mut();
        let network = guest.instance_network_instance_network().unwrap();
        let wrong = guest.tcp_tcp_socket_drop(TcpSocket(network.0)).is_err();
        let kept = guest.ensure_network(network).is_ok();
        assert!(
            wrong && kept,
            "tcp-socket's drop of a network handle must trap and leave the network held"
        );
    }
}
