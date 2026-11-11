//! What the Wasm Driver answers each preview2 import. Without a network Grant
//! the guest makes no socket, as WASI says a host without a network answers;
//! with one, each connect passes the Invocation's ruleset and dials through
//! its Protocol Family. The clock subscriptions, the streams, and `poll` wait
//! on the runtime's wait primitive.

use std::collections::BTreeMap;
use std::net::{Shutdown, SocketAddr};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use kobako_runtime::net::{
    sock_alloc_file, Namespace, ProtoOps, SockOpt, MAX_DATAGRAM, MAX_TCP_KEEPCNT, MAX_TCP_KEEPIDLE,
    MAX_TCP_KEEPINTVL,
};
use kobako_runtime::vfs::{Cred, FilesystemError, PollMask};
use kobako_runtime::wait::{do_poll, PollTable};

use super::bindings::wasi_clocks_monotonic_clock_0_2_12 as monotonic_clock;
use super::bindings::wasi_io_error_0_2_12::Error;
use super::bindings::wasi_io_poll_0_2_12::Pollable;
use super::bindings::wasi_io_streams_0_2_12::{self as streams, InputStream, OutputStream};
use super::bindings::wasi_sockets_ip_name_lookup_0_2_12::ResolveAddressStream;
use super::bindings::wasi_sockets_network_0_2_12::{
    ErrorCode, IpAddress, IpAddressFamily, IpSocketAddress, Network,
};
use super::bindings::wasi_sockets_tcp_0_2_12::{ShutdownType, TcpSocket};
use super::bindings::wasi_sockets_udp_0_2_12::{
    IncomingDatagram, IncomingDatagramStream, OutgoingDatagram, OutgoingDatagramStream, UdpSocket,
};
use super::bindings::{ClocksHost, IoHost, SocketsHost};
use super::error::{SocketError, StreamError};
use super::sockets::{
    from_socket_addr, is_valid_unicast_address, to_asked_size, to_buffer_size, to_error_code,
    to_hop_limit, to_keepalive_secs, to_lookup_error_code, to_nanos, to_remote_addr,
    to_socket_addr,
};
use super::table::SharedFile;
use super::table::{self, EndingStream, SocketFile, Stream, TcpState, UdpState};
use super::MAX_READ_SIZE_ALLOC;
use crate::invocation::Invocation;
use crate::trap::{Interrupted, TimeoutTrap};

/// The most one `blocking-write-and-flush` takes, past which `io.wit` says the
/// call traps.
const MAX_BLOCKING_WRITE: usize = 4096;

/// The most datagrams one `receive` answers and one `check-send` permits, as
/// wasmtime-wasi's.
const MAX_DATAGRAMS: usize = 16;

/// Where a socket stands for a bind, as each protocol reads its own state.
enum BindStanding {
    Unbound,
    /// A bind or a connect is under way.
    Busy,
    /// Bound, connected, or closed.
    Settled,
}

impl Invocation {
    /// A pollable on the guest's own clocks, `nanos` of host time from now;
    /// the wait holding it decides when it is due.
    fn subscribe_clock(&mut self, nanos: u64) -> wasmtime::Result<Pollable> {
        self.table_mut().push(table::Pollable::Clock {
            at: Instant::now() + Duration::from_nanos(nanos),
        })
    }

    pub(super) fn table_mut(&mut self) -> &mut table::Table {
        self.wasi_mut().table_mut()
    }

    /// What `opt` reads of the TCP socket `this`, in a state that takes an
    /// option.
    fn tcp_option(&mut self, this: TcpSocket, opt: SockOpt) -> Result<u32, SocketError> {
        let socket = self.tcp_mut(this)?;
        socket.state.ensure_takes_options()?;
        Ok(socket.socket.sock.getsockopt(opt)?)
    }

    /// Set `opt` of the TCP socket `this` to `value`, in a state that takes
    /// an option.
    fn set_tcp_option(
        &mut self,
        this: TcpSocket,
        opt: SockOpt,
        value: u32,
    ) -> Result<(), SocketError> {
        let socket = self.tcp_mut(this)?;
        socket.state.ensure_takes_options()?;
        Ok(socket.socket.sock.setsockopt(opt, value)?)
    }

    /// Set the buffer size `opt` of the TCP socket `this` to `value`; a
    /// socket that takes no option answers before zero does, as wasmtime
    /// reads the socket before the size.
    fn set_tcp_buffer_size(
        &mut self,
        this: TcpSocket,
        opt: SockOpt,
        value: u64,
    ) -> Result<(), SocketError> {
        let socket = self.tcp_mut(this)?;
        socket.state.ensure_takes_options()?;
        let bytes = to_buffer_size(value).ok_or(ErrorCode::InvalidArgument)?;
        Ok(socket.socket.sock.setsockopt(opt, bytes)?)
    }

    /// What `opt` reads of the UDP socket `this`, which takes an option in
    /// any state, as wasmtime's does.
    fn udp_option(&mut self, this: UdpSocket, opt: SockOpt) -> Result<u32, SocketError> {
        Ok(self.udp_mut(this)?.socket.sock.getsockopt(opt)?)
    }

    /// Set `opt` of the UDP socket `this` to `value`.
    fn set_udp_option(
        &mut self,
        this: UdpSocket,
        opt: SockOpt,
        value: u32,
    ) -> Result<(), SocketError> {
        Ok(self.udp_mut(this)?.socket.sock.setsockopt(opt, value)?)
    }

    /// The indices of `pollables` ready, once one is, as WASI's `poll` finds
    /// them: each clock is due as the profile has it beside any host-side
    /// producer among them. Both encodings wait here, so preview1's
    /// `poll_oneoff` answers by the same rule.
    pub(crate) fn poll_ready(&self, pollables: Vec<table::Pollable>) -> wasmtime::Result<Vec<u32>> {
        let has_producer = pollables.iter().any(table::Pollable::is_producer);
        let profile = self.profile();
        let pollables = pollables
            .into_iter()
            .map(|pollable| pollable.for_wait(profile, has_producer))
            .collect::<Vec<_>>();
        let due = pollables.iter().filter_map(table::Pollable::due).min();
        self.wait_for(due, |mut table| {
            let ready = (0..pollables.len() as u32)
                .filter(|i| pollables[*i as usize].is_ready(table.as_deref_mut()))
                .collect::<Vec<_>>();
            (!ready.is_empty()).then_some(ready)
        })
    }

    /// Wait until `ready` answers, or end with `TimeoutTrap` at the deadline
    /// and with `Interrupted` as the frontend interrupts; the wait sleeps
    /// until the soonest pollable is due, or a queue `ready` registered on
    /// wakes.
    fn wait_for<T>(
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
                Err(_) => return Err(wasmtime::Error::new(Interrupted)),
            }
        }
    }

    /// The Invocation's network, which every socket and lookup is made in.
    fn network(&self) -> &Namespace {
        self.wasi()
            .network()
            .expect("a socket or a lookup is only made with a network")
    }

    /// A socket `create` makes in the Invocation's network, with its file,
    /// handed out as the handle `resource` takes, as `__sys_socket` makes one.
    /// With no network it is `access-denied`, as wasmtime-wasi's host with
    /// sockets disallowed answers; for an IPv6 `family` `not-supported`, and
    /// past the open file limit `new-socket-limit`.
    fn create_socket<S: ProtoOps + 'static, H: table::Handle>(
        &mut self,
        family: IpAddressFamily,
        create: impl FnOnce(&Namespace, &Arc<Cred>) -> Result<Arc<S>, FilesystemError>,
        resource: impl FnOnce(SocketFile<S>) -> H::Host,
    ) -> Result<H, SocketError> {
        if self.wasi().network().is_none() {
            return Err(ErrorCode::AccessDenied.into());
        }
        if family == IpAddressFamily::Ipv6 {
            return Err(ErrorCode::NotSupported.into());
        }
        if self.wasi().table().is_full() {
            return Err(ErrorCode::NewSocketLimit.into());
        }
        let cred = self.wasi().cred().clone();
        let sock = create(self.network(), &cred)?;
        let socket = SocketFile::new(sock.clone(), sock_alloc_file(sock, &cred));
        Ok(self.table_mut().push_file(resource(socket))?)
    }

    /// A pollable ready once `file`'s poll finds `mask`, a child `parent` must
    /// outlive, as every pollable on a resource is.
    pub(super) fn subscribe_file(
        &mut self,
        file: SharedFile,
        mask: PollMask,
        parent: impl table::Handle,
    ) -> wasmtime::Result<Pollable> {
        self.table_mut()
            .push_child(table::Pollable::File(file, mask), parent)
    }

    /// Start a bind of `sock`, standing at `standing`, to `local_address`, as
    /// both sockets' `start-bind` do in wasmtime's order, the network handle
    /// checked as it was lifted: a bind or connect under way, then any other
    /// state, then the address as WASI takes it and the protocol finds
    /// `is_valid`, through the socket hooks to the socket's own bind.
    fn start_bind(
        &mut self,
        sock: &dyn ProtoOps,
        local_address: &IpSocketAddress,
        standing: BindStanding,
        is_valid: fn(SocketAddr) -> bool,
    ) -> Result<(), SocketError> {
        match standing {
            BindStanding::Unbound => {}
            BindStanding::Busy => return Err(ErrorCode::ConcurrencyConflict.into()),
            BindStanding::Settled => return Err(ErrorCode::InvalidState.into()),
        }
        let addr = to_socket_addr(local_address)
            .filter(|addr| is_valid(*addr))
            .ok_or(ErrorCode::InvalidArgument)?;
        let cred = self.wasi().cred().clone();
        Ok(self.network().bind(&cred, sock, addr)?)
    }

    fn tcp_mut(&mut self, handle: TcpSocket) -> wasmtime::Result<&mut table::TcpSocket> {
        self.table_mut().get_mut(handle)
    }

    fn udp_mut(&mut self, handle: UdpSocket) -> wasmtime::Result<&mut table::UdpSocket> {
        self.table_mut().get_mut(handle)
    }

    /// The permit an outgoing datagram stream's last `check-send` gave.
    fn permit_mut(&mut self, stream: OutgoingDatagramStream) -> wasmtime::Result<&mut u64> {
        Ok(&mut self.table_mut().get_mut(stream)?.permit)
    }

    /// Wait until `stream`'s poll finds `mask`, or an error or a hang-up.
    fn wait_on(&self, stream: &Stream, mask: PollMask) -> wasmtime::Result<()> {
        let pollable = table::Pollable::File(stream.shared_file(), mask);
        self.wait_for(None, |table| pollable.is_ready(table).then_some(()))
    }

    /// `call` on a stream still open: one closed answers `closed`, and a call
    /// that fails or answers `closed` closes it, as `io.wit` has every
    /// operation past `last-operation-failed` answer `closed`.
    fn within_open<H, T>(
        &mut self,
        this: H,
        call: impl FnOnce(&mut Self) -> Result<T, StreamError>,
    ) -> Result<T, StreamError>
    where
        H: table::Handle,
        H::Host: EndingStream,
    {
        if self.table_mut().get(this)?.closed().load(Ordering::Acquire) {
            return Err(StreamError::Closed);
        }
        let answer = call(self);
        if let Err(StreamError::Closed | StreamError::LastOperationFailed(_)) = answer {
            self.table_mut()
                .get(this)?
                .closed()
                .store(true, Ordering::Release);
        }
        answer
    }
}

impl SocketsHost for Invocation {
    fn network_convert_error_code(&mut self, err: SocketError) -> wasmtime::Result<ErrorCode> {
        err.downcast()
    }

    fn instance_network_instance_network(&mut self) -> wasmtime::Result<Network> {
        self.table_mut().push(table::Network)
    }

    fn network_network_drop(&mut self, handle: Network) -> wasmtime::Result<()> {
        self.table_mut().delete(handle).map(drop)
    }

    /// A lookup of `name`, answered through the stream it hands out. Without a
    /// network Grant no resolver answers, as WASI's `permanent-resolver-failure`
    /// says of a host without one.
    fn ip_name_lookup_resolve_addresses(
        &mut self,
        _network: Network,
        name: String,
    ) -> Result<ResolveAddressStream, SocketError> {
        let Some(namespace) = self.wasi().network() else {
            return Err(ErrorCode::PermanentResolverFailure.into());
        };
        if self.wasi().table().is_full() {
            return Err(ErrorCode::NewSocketLimit.into());
        }
        let lookup = namespace
            .resolve(&name)
            .map_err(|err| to_lookup_error_code(err, true))?;
        let file = lookup.alloc_file(self.wasi().cred());
        Ok(self.table_mut().push_file(table::ResolveAddressStream {
            lookup,
            file: Arc::new(Mutex::new(file)),
            handed: false,
        })?)
    }

    /// The name's one virtual address, then the end; a lookup still under way
    /// answers `would-block`.
    fn ip_name_lookup_resolve_address_stream_resolve_next_address(
        &mut self,
        this: ResolveAddressStream,
    ) -> Result<Option<IpAddress>, SocketError> {
        let stream = self.table_mut().get(this)?;
        if stream.handed {
            return Ok(None);
        }
        let lookup = stream.lookup.clone();
        let cred = self.wasi().cred();
        let at = match self.network().alloc_virtual_address(cred, &lookup) {
            None => return Err(ErrorCode::WouldBlock.into()),
            Some(answer) => answer.map_err(|err| to_lookup_error_code(err, false))?,
        };
        self.table_mut().get_mut(this)?.handed = true;
        let [a, b, c, d] = at.octets();
        Ok(Some(IpAddress::Ipv4((a, b, c, d))))
    }

    fn ip_name_lookup_resolve_address_stream_subscribe(
        &mut self,
        this: ResolveAddressStream,
    ) -> wasmtime::Result<Pollable> {
        let file = self.table_mut().get(this)?.file.clone();
        self.subscribe_file(file, PollMask::IN, this)
    }

    fn ip_name_lookup_resolve_address_stream_drop(
        &mut self,
        handle: ResolveAddressStream,
    ) -> wasmtime::Result<()> {
        self.table_mut().delete(handle).map(drop)
    }

    /// Without a network Grant no socket is made, as WASI's `not-supported`
    /// says of a host without one; the guest is IPv4-only, so IPv6 is not
    /// supported either.
    fn tcp_create_socket_create_tcp_socket(
        &mut self,
        address_family: IpAddressFamily,
    ) -> Result<TcpSocket, SocketError> {
        self.create_socket(address_family, Namespace::create_tcp_socket, |socket| {
            table::TcpSocket {
                socket,
                state: TcpState::Unbound,
                input_closed: Arc::default(),
            }
        })
    }

    /// A bind the socket hooks pass, as `tcp-socket.start-bind` asks; the
    /// host socket takes its own address as it connects, so a bind settles
    /// at once.
    fn tcp_tcp_socket_start_bind(
        &mut self,
        this: TcpSocket,
        _network: Network,
        local_address: IpSocketAddress,
    ) -> Result<(), SocketError> {
        let socket = self.tcp_mut(this)?;
        let standing = match socket.state {
            TcpState::Unbound => BindStanding::Unbound,
            state if state.is_in_progress() => BindStanding::Busy,
            _ => BindStanding::Settled,
        };
        let sock = socket.socket.sock.clone();
        self.start_bind(
            sock.as_ref(),
            &local_address,
            standing,
            is_valid_unicast_address,
        )?;
        self.tcp_mut(this)?.state = TcpState::Binding;
        Ok(())
    }

    fn tcp_tcp_socket_finish_bind(&mut self, this: TcpSocket) -> Result<(), SocketError> {
        let socket = self.tcp_mut(this)?;
        if socket.state != TcpState::Binding {
            return Err(ErrorCode::NotInProgress.into());
        }
        socket.state = TcpState::Bound;
        Ok(())
    }

    /// A connect passes the Invocation's namespace on what the guest named,
    /// which dials that exact address without blocking, so this answers at
    /// once.
    fn tcp_tcp_socket_start_connect(
        &mut self,
        this: TcpSocket,
        _network: Network,
        remote_address: IpSocketAddress,
    ) -> Result<(), SocketError> {
        let socket = self.tcp_mut(this)?;
        match socket.state {
            TcpState::Unbound | TcpState::Bound => {}
            state if state.is_in_progress() => return Err(ErrorCode::ConcurrencyConflict.into()),
            _ => return Err(ErrorCode::InvalidState.into()),
        }
        let sock = socket.socket.sock.clone();
        let addr = to_remote_addr(&remote_address)
            .filter(|addr| is_valid_unicast_address(*addr))
            .ok_or(ErrorCode::InvalidArgument)?;
        let cred = self.wasi().cred();
        // Past the address's own checks the attempt is under way, so its
        // failure waits for `finish-connect`, as wasmtime's connecting future
        // checks permission and dials, and then closes the socket. A full
        // connection count is refused at once, as `would-block` from
        // `finish-connect` would read as a connect still under way.
        self.tcp_mut(this)?.state = match self.network().connect(cred, &sock, addr) {
            Ok(()) => TcpState::Connecting,
            Err(FilesystemError::WouldBlock) => {
                self.tcp_mut(this)?.state = TcpState::Closed(ErrorCode::WouldBlock);
                return Err(ErrorCode::WouldBlock.into());
            }
            Err(err) => TcpState::ConnectFailed(to_error_code(&err)),
        };
        Ok(())
    }

    /// The streams of a connect that landed, as WASI hands them out once; one
    /// still under way answers `would-block`, and a failed one closes the
    /// socket.
    fn tcp_tcp_socket_finish_connect(
        &mut self,
        this: TcpSocket,
    ) -> Result<(InputStream, OutputStream), SocketError> {
        let socket = self.tcp_mut(this)?;
        let finished = match socket.state {
            TcpState::Connecting => socket.socket.sock.finish_connect(),
            TcpState::ConnectFailed(code) => {
                socket.state = TcpState::Closed(code);
                return Err(code.into());
            }
            _ => return Err(ErrorCode::NotInProgress.into()),
        };
        let (socket, input_closed) = (socket.socket.clone(), socket.input_closed.clone());
        match finished {
            Ok(()) => {
                self.tcp_mut(this)?.state = TcpState::Connected;
                let stream = Stream::Socket(socket);
                let input = self.table_mut().push_child(
                    table::InputStream::with_closed(stream.clone(), input_closed),
                    this,
                )?;
                let output = self
                    .table_mut()
                    .push_child(table::OutputStream::new(stream), this)?;
                Ok((input, output))
            }
            Err(FilesystemError::WouldBlock) => Err(ErrorCode::WouldBlock.into()),
            Err(err) => {
                let code = to_error_code(&err);
                self.tcp_mut(this)?.state = TcpState::Closed(code);
                Err(code.into())
            }
        }
    }

    /// Waits on a connect under way whenever it is polled, and is ready
    /// otherwise, as wasmtime's socket pollable waits only on an operation in
    /// progress.
    fn tcp_tcp_socket_subscribe(&mut self, this: TcpSocket) -> wasmtime::Result<Pollable> {
        let socket = self.tcp_mut(this)?.socket.clone();
        self.table_mut()
            .push_child(table::Pollable::TcpSocket(socket), this)
    }

    fn tcp_tcp_socket_shutdown(
        &mut self,
        this: TcpSocket,
        shutdown_type: ShutdownType,
    ) -> Result<(), SocketError> {
        let socket = self.tcp_mut(this)?;
        if socket.state != TcpState::Connected {
            return Err(ErrorCode::InvalidState.into());
        }
        let how = match shutdown_type {
            ShutdownType::Receive => Shutdown::Read,
            ShutdownType::Send => Shutdown::Write,
            ShutdownType::Both => Shutdown::Both,
        };
        // `tcp.wit` closes the input stream and drops what it still queued,
        // where Linux's `SHUT_RD` keeps reading what arrived.
        if how != Shutdown::Write {
            socket.input_closed.store(true, Ordering::Release);
        }
        Ok(socket.socket.sock.shutdown(how)?)
    }

    /// The guest's own end once connected, at the virtual port the namespace
    /// gave it, and the unspecified end it bound before that. As wasmtime's,
    /// a socket unbound or still connecting answers `invalid-state`, and one
    /// whose connect failed answers its error.
    fn tcp_tcp_socket_local_address(
        &mut self,
        this: TcpSocket,
    ) -> Result<IpSocketAddress, SocketError> {
        let socket = self.tcp_mut(this)?;
        match socket.state {
            TcpState::Connected | TcpState::Binding | TcpState::Bound => {
                Ok(from_socket_addr(socket.socket.sock.getsockname()))
            }
            TcpState::Closed(code) => Err(code.into()),
            TcpState::Unbound | TcpState::Connecting | TcpState::ConnectFailed(_) => {
                Err(ErrorCode::InvalidState.into())
            }
        }
    }

    /// The peer as the guest named it.
    fn tcp_tcp_socket_remote_address(
        &mut self,
        this: TcpSocket,
    ) -> Result<IpSocketAddress, SocketError> {
        let socket = self.tcp_mut(this)?;
        match (socket.state, socket.socket.sock.getpeername()) {
            (TcpState::Connected, Some(peer)) => Ok(from_socket_addr(peer)),
            (TcpState::Closed(code), _) => Err(code.into()),
            _ => Err(ErrorCode::InvalidState.into()),
        }
    }

    /// Whether the socket sends keepalive probes, from the socket's own
    /// setting.
    fn tcp_tcp_socket_keep_alive_enabled(&mut self, this: TcpSocket) -> Result<bool, SocketError> {
        Ok(self.tcp_option(this, SockOpt::KeepAlive)? != 0)
    }

    fn tcp_tcp_socket_set_keep_alive_enabled(
        &mut self,
        this: TcpSocket,
        value: bool,
    ) -> Result<(), SocketError> {
        self.set_tcp_option(this, SockOpt::KeepAlive, u32::from(value))
    }

    fn tcp_tcp_socket_keep_alive_idle_time(&mut self, this: TcpSocket) -> Result<u64, SocketError> {
        Ok(to_nanos(self.tcp_option(this, SockOpt::KeepIdle)?))
    }

    /// As wasmtime's, zero answers `invalid-argument` and any other time is
    /// clamped to what Linux takes.
    fn tcp_tcp_socket_set_keep_alive_idle_time(
        &mut self,
        this: TcpSocket,
        value: u64,
    ) -> Result<(), SocketError> {
        let secs = to_keepalive_secs(value, MAX_TCP_KEEPIDLE).ok_or(ErrorCode::InvalidArgument)?;
        self.set_tcp_option(this, SockOpt::KeepIdle, secs)
    }

    fn tcp_tcp_socket_keep_alive_interval(&mut self, this: TcpSocket) -> Result<u64, SocketError> {
        Ok(to_nanos(self.tcp_option(this, SockOpt::KeepIntvl)?))
    }

    fn tcp_tcp_socket_set_keep_alive_interval(
        &mut self,
        this: TcpSocket,
        value: u64,
    ) -> Result<(), SocketError> {
        let secs = to_keepalive_secs(value, MAX_TCP_KEEPINTVL).ok_or(ErrorCode::InvalidArgument)?;
        self.set_tcp_option(this, SockOpt::KeepIntvl, secs)
    }

    fn tcp_tcp_socket_keep_alive_count(&mut self, this: TcpSocket) -> Result<u32, SocketError> {
        self.tcp_option(this, SockOpt::KeepCnt)
    }

    /// As wasmtime's, zero answers `invalid-argument` and any other count is
    /// clamped to Linux's ceiling.
    fn tcp_tcp_socket_set_keep_alive_count(
        &mut self,
        this: TcpSocket,
        value: u32,
    ) -> Result<(), SocketError> {
        if value == 0 {
            return Err(ErrorCode::InvalidArgument.into());
        }
        self.set_tcp_option(this, SockOpt::KeepCnt, value.min(MAX_TCP_KEEPCNT))
    }

    /// Half what the receive queue holds, as wasmtime answers it on Linux,
    /// so a guest reads back about the size it asked for.
    fn tcp_tcp_socket_receive_buffer_size(&mut self, this: TcpSocket) -> Result<u64, SocketError> {
        Ok(to_asked_size(self.tcp_option(this, SockOpt::RcvBuf)?))
    }

    fn tcp_tcp_socket_set_receive_buffer_size(
        &mut self,
        this: TcpSocket,
        value: u64,
    ) -> Result<(), SocketError> {
        self.set_tcp_buffer_size(this, SockOpt::RcvBuf, value)
    }

    fn tcp_tcp_socket_send_buffer_size(&mut self, this: TcpSocket) -> Result<u64, SocketError> {
        Ok(to_asked_size(self.tcp_option(this, SockOpt::SndBuf)?))
    }

    fn tcp_tcp_socket_set_send_buffer_size(
        &mut self,
        this: TcpSocket,
        value: u64,
    ) -> Result<(), SocketError> {
        self.set_tcp_buffer_size(this, SockOpt::SndBuf, value)
    }

    fn tcp_tcp_socket_hop_limit(&mut self, this: TcpSocket) -> Result<u8, SocketError> {
        Ok(to_hop_limit(self.tcp_option(this, SockOpt::Ttl)?))
    }

    /// Zero answers `invalid-argument`, as the runtime refuses it.
    fn tcp_tcp_socket_set_hop_limit(
        &mut self,
        this: TcpSocket,
        value: u8,
    ) -> Result<(), SocketError> {
        self.set_tcp_option(this, SockOpt::Ttl, u32::from(value))
    }

    fn tcp_tcp_socket_drop(&mut self, handle: TcpSocket) -> wasmtime::Result<()> {
        self.table_mut().delete(handle).map(drop)
    }

    /// Without a network Grant no socket is made, and the guest is IPv4-only,
    /// as a TCP socket's creation answers.
    fn udp_create_socket_create_udp_socket(
        &mut self,
        address_family: IpAddressFamily,
    ) -> Result<UdpSocket, SocketError> {
        self.create_socket(address_family, Namespace::create_udp_socket, |socket| {
            table::UdpSocket {
                socket,
                state: UdpState::Unbound,
            }
        })
    }

    /// The guest only sends to a peer it names, so the one bind it makes is
    /// the implicit one, to any address on an ephemeral port; any other is
    /// refused, as Landlock's `BIND_*` rights refuse one.
    fn udp_udp_socket_start_bind(
        &mut self,
        this: UdpSocket,
        _network: Network,
        local_address: IpSocketAddress,
    ) -> Result<(), SocketError> {
        let socket = self.udp_mut(this)?;
        let standing = match socket.state {
            UdpState::Unbound => BindStanding::Unbound,
            UdpState::Binding => BindStanding::Busy,
            _ => BindStanding::Settled,
        };
        let sock = socket.socket.sock.clone();
        // UDP binds any address WASI takes, as wasmtime asks UDP of its family
        // alone; the socket hooks refuse all but the implicit one.
        self.start_bind(sock.as_ref(), &local_address, standing, |_| true)?;
        self.udp_mut(this)?.state = UdpState::Binding;
        Ok(())
    }

    fn udp_udp_socket_finish_bind(&mut self, this: UdpSocket) -> Result<(), SocketError> {
        let socket = self.udp_mut(this)?;
        if socket.state != UdpState::Binding {
            return Err(ErrorCode::NotInProgress.into());
        }
        socket.state = UdpState::Bound;
        Ok(())
    }

    /// The guest's own end, once bound, at the virtual port the namespace gave
    /// it as it connected; an unbound socket answers `invalid-state`, as
    /// wasmtime's.
    fn udp_udp_socket_local_address(
        &mut self,
        this: UdpSocket,
    ) -> Result<IpSocketAddress, SocketError> {
        let socket = self.udp_mut(this)?;
        if !socket.state.is_bound() {
            return Err(ErrorCode::InvalidState.into());
        }
        Ok(from_socket_addr(socket.socket.sock.getsockname()))
    }

    /// The hop limit, from the socket's own setting; a UDP socket takes an
    /// option in any state, as wasmtime's does.
    fn udp_udp_socket_unicast_hop_limit(&mut self, this: UdpSocket) -> Result<u8, SocketError> {
        Ok(to_hop_limit(self.udp_option(this, SockOpt::Ttl)?))
    }

    fn udp_udp_socket_set_unicast_hop_limit(
        &mut self,
        this: UdpSocket,
        value: u8,
    ) -> Result<(), SocketError> {
        self.set_udp_option(this, SockOpt::Ttl, u32::from(value))
    }

    fn udp_udp_socket_receive_buffer_size(&mut self, this: UdpSocket) -> Result<u64, SocketError> {
        Ok(to_asked_size(self.udp_option(this, SockOpt::RcvBuf)?))
    }

    fn udp_udp_socket_set_receive_buffer_size(
        &mut self,
        this: UdpSocket,
        value: u64,
    ) -> Result<(), SocketError> {
        let bytes = to_buffer_size(value).ok_or(ErrorCode::InvalidArgument)?;
        self.set_udp_option(this, SockOpt::RcvBuf, bytes)
    }

    fn udp_udp_socket_send_buffer_size(&mut self, this: UdpSocket) -> Result<u64, SocketError> {
        Ok(to_asked_size(self.udp_option(this, SockOpt::SndBuf)?))
    }

    fn udp_udp_socket_set_send_buffer_size(
        &mut self,
        this: UdpSocket,
        value: u64,
    ) -> Result<(), SocketError> {
        let bytes = to_buffer_size(value).ok_or(ErrorCode::InvalidArgument)?;
        self.set_udp_option(this, SockOpt::SndBuf, bytes)
    }

    /// The streams to the one peer the guest names, which passes the
    /// Invocation's namespace as a connect does. A stream with no peer would
    /// send each datagram anywhere, so it is refused.
    fn udp_udp_socket_stream(
        &mut self,
        this: UdpSocket,
        remote_address: Option<IpSocketAddress>,
    ) -> Result<(IncomingDatagramStream, OutgoingDatagramStream), SocketError> {
        let table = self.table_mut();
        if table.has_child::<IncomingDatagramStream>(this)
            || table.has_child::<OutgoingDatagramStream>(this)
        {
            return Err(wasmtime::Error::msg("UDP streams not dropped yet").into());
        }
        let socket = self.udp_mut(this)?;
        if !socket.state.is_bound() {
            return Err(ErrorCode::InvalidState.into());
        }
        let socket_file = socket.socket.clone();
        let peer = match remote_address.as_ref().map(to_remote_addr) {
            Some(Some(addr)) => Some(addr),
            Some(None) => return Err(ErrorCode::InvalidArgument.into()),
            None => None,
        };
        let cred = self.wasi().cred();
        self.network()
            .connect_datagram(cred, &socket_file.sock, peer)?;
        let incoming = self
            .table_mut()
            .push_child(table::IncomingDatagramStream(socket_file.clone()), this)?;
        let outgoing = self.table_mut().push_child(
            table::OutgoingDatagramStream {
                socket: socket_file,
                permit: 0,
            },
            this,
        )?;
        Ok((incoming, outgoing))
    }

    /// Ready at once, since a bind settles as it starts.
    fn udp_udp_socket_subscribe(&mut self, this: UdpSocket) -> wasmtime::Result<Pollable> {
        self.table_mut().push_child(table::Pollable::Ready, this)
    }

    fn udp_udp_socket_drop(&mut self, handle: UdpSocket) -> wasmtime::Result<()> {
        self.table_mut().delete(handle).map(drop)
    }

    /// Up to `max_results` datagrams the peer sent, without waiting, never
    /// past `MAX_DATAGRAMS` or about `MAX_READ_SIZE_ALLOC` bytes, each
    /// datagram counting one more for its record; none yet is an empty list.
    fn udp_incoming_datagram_stream_receive(
        &mut self,
        this: IncomingDatagramStream,
        max_results: u64,
    ) -> Result<Vec<IncomingDatagram>, SocketError> {
        let socket_file = self.table_mut().get(this)?.0.clone();
        let max_results =
            usize::try_from(max_results).map_or(MAX_DATAGRAMS, |max| max.min(MAX_DATAGRAMS));
        let mut received = Vec::new();
        let mut sum = 0;
        let mut buf = vec![0; MAX_DATAGRAM];
        while received.len() < max_results && sum < MAX_READ_SIZE_ALLOC {
            match socket_file.sock.recvmsg(&mut buf) {
                Ok((n, Some(from))) => {
                    sum += 1 + n;
                    received.push(IncomingDatagram {
                        data: buf[..n].to_vec(),
                        remote_address: from_socket_addr(from),
                    });
                }
                Ok((_, None)) => unreachable!("a UDP socket names each datagram's sender"),
                Err(FilesystemError::WouldBlock) => break,
                Err(err) if received.is_empty() => return Err(err.into()),
                Err(_) => break,
            }
        }
        Ok(received)
    }

    fn udp_incoming_datagram_stream_subscribe(
        &mut self,
        this: IncomingDatagramStream,
    ) -> wasmtime::Result<Pollable> {
        let file = self.table_mut().get(this)?.0.shared_file();
        self.subscribe_file(file, PollMask::IN, this)
    }

    fn udp_incoming_datagram_stream_drop(
        &mut self,
        handle: IncomingDatagramStream,
    ) -> wasmtime::Result<()> {
        self.table_mut().delete(handle).map(drop)
    }

    /// `MAX_DATAGRAMS` whenever the stream's pollable is ready, none
    /// otherwise, as wasmtime-wasi's answers both from one readiness; a
    /// pending failure permits, so the `send` it permits answers it, and a
    /// `send` stops where the queue fills.
    fn udp_outgoing_datagram_stream_check_send(
        &mut self,
        this: OutgoingDatagramStream,
    ) -> Result<u64, SocketError> {
        let socket_file = self.table_mut().get(this)?.socket.clone();
        let found = socket_file.file().poll(None);
        let permitted = if found.is_ready_for(PollMask::OUT) {
            MAX_DATAGRAMS as u64
        } else {
            0
        };
        *self.permit_mut(this)? = permitted;
        Ok(permitted)
    }

    /// The datagrams sent in order, until the queue is full; one addressed to
    /// another peer, or past what UDP carries, is refused. More than the last
    /// `check-send` permitted traps, as `udp.wit` says it must, and a send
    /// spends the permit.
    fn udp_outgoing_datagram_stream_send(
        &mut self,
        this: OutgoingDatagramStream,
        datagrams: Vec<OutgoingDatagram>,
    ) -> Result<u64, SocketError> {
        if datagrams.is_empty() {
            return Ok(0);
        }
        let permit = std::mem::take(self.permit_mut(this)?);
        if datagrams.len() as u64 > permit {
            return Err(wasmtime::Error::msg(
                "the guest sent more datagrams than check-send permitted",
            )
            .into());
        }
        let socket_file = self.table_mut().get(this)?.socket.clone();
        let mut sent = 0;
        for datagram in datagrams {
            let addressed = datagram.remote_address.as_ref().map(to_socket_addr);
            // Its size is judged before its address, as wasmtime's send asks.
            let refused = if datagram.data.len() > MAX_DATAGRAM {
                Some(ErrorCode::DatagramTooLarge)
            } else if addressed.is_some_and(|addr| addr != socket_file.sock.getpeername()) {
                Some(ErrorCode::InvalidArgument)
            } else {
                match socket_file.file().write(&datagram.data) {
                    Ok(_) => None,
                    Err(FilesystemError::WouldBlock) => break,
                    Err(err) => Some(to_error_code(&err)),
                }
            };
            match refused {
                None => sent += 1,
                Some(code) if sent == 0 => return Err(code.into()),
                Some(_) => break,
            }
        }
        Ok(sent)
    }

    fn udp_outgoing_datagram_stream_subscribe(
        &mut self,
        this: OutgoingDatagramStream,
    ) -> wasmtime::Result<Pollable> {
        let file = self.table_mut().get(this)?.socket.shared_file();
        self.subscribe_file(file, PollMask::OUT, this)
    }

    fn udp_outgoing_datagram_stream_drop(
        &mut self,
        handle: OutgoingDatagramStream,
    ) -> wasmtime::Result<()> {
        self.table_mut().delete(handle).map(drop)
    }
}

impl IoHost for Invocation {
    /// The end as `closed`, a trap as itself, and any other failure as an
    /// error the guest may read, as wasmtime-wasi-io hands one out.
    fn streams_convert_stream_error(
        &mut self,
        err: StreamError,
    ) -> wasmtime::Result<streams::StreamError> {
        match err {
            StreamError::Closed => Ok(streams::StreamError::Closed),
            StreamError::LastOperationFailed(err) => Ok(streams::StreamError::LastOperationFailed(
                self.table_mut().push(table::Error(err))?,
            )),
            StreamError::Trap(trap) => Err(trap),
        }
    }

    /// What the stream holds now, up to `len`, without waiting: nothing yet
    /// reads as an empty list, and the end as `closed`.
    fn streams_input_stream_read(
        &mut self,
        this: InputStream,
        len: u64,
    ) -> Result<Vec<u8>, StreamError> {
        self.within_open(this, |host| {
            let stream = host.table_mut().get(this)?.stream.clone();
            let mut buf = vec![0; len.min(MAX_READ_SIZE_ALLOC as u64) as usize];
            match stream.read(&mut buf) {
                Ok(0) if !buf.is_empty() => Err(StreamError::Closed),
                Ok(n) => {
                    buf.truncate(n);
                    Ok(buf)
                }
                Err(FilesystemError::WouldBlock) => Ok(Vec::new()),
                Err(err) => Err(err.into()),
            }
        })
    }

    /// A pollable the stream must outlive, as `io.wit` makes it a child.
    fn streams_input_stream_subscribe(&mut self, this: InputStream) -> wasmtime::Result<Pollable> {
        let file = self.table_mut().get(this)?.stream.shared_file();
        self.subscribe_file(file, PollMask::IN, this)
    }

    fn streams_input_stream_drop(&mut self, handle: InputStream) -> wasmtime::Result<()> {
        self.table_mut().delete(handle).map(drop)
    }

    /// Write every byte, at most 4096, waiting for room as it goes, then until
    /// what was written has left for the host. A stream that closed once every
    /// byte was taken answers success, as wasmtime-wasi-io's does; a failure
    /// still answers, and so does a stream closed before a write of nothing,
    /// as wasmtime's first asks the stream for room.
    fn streams_output_stream_blocking_write_and_flush(
        &mut self,
        this: OutputStream,
        contents: Vec<u8>,
    ) -> Result<(), StreamError> {
        if contents.len() > MAX_BLOCKING_WRITE {
            return Err(wasmtime::Error::msg(
                "the guest handed blocking-write-and-flush more than 4096 bytes",
            )
            .into());
        }
        self.within_open(this, |host| {
            let stream = host.table_mut().get(this)?.stream.clone();
            let mut rest = &contents[..];
            while !rest.is_empty() {
                host.wait_on(&stream, PollMask::OUT)?;
                match stream.write(rest) {
                    Ok(n) => rest = &rest[n..],
                    Err(FilesystemError::WouldBlock) => {}
                    Err(err) => return Err(err.into()),
                }
            }
            // Registering through the stream's poll puts the wait on its
            // queue, which a socket's writer wakes after each chunk it sends.
            host.wait_for(None, |table| {
                let failed = stream.poll(table).contains(PollMask::ERR);
                (failed || stream.is_drained()).then_some(())
            })?;
            match stream.write(&[]) {
                Err(FilesystemError::BrokenPipe) if !contents.is_empty() => Ok(()),
                Ok(_) => Ok(()),
                Err(err) => Err(err.into()),
            }
        })
    }

    /// How many bytes a write takes now, zero while the stream is full or a
    /// flush is under way; a failed stream answers its failure.
    fn streams_output_stream_check_write(
        &mut self,
        this: OutputStream,
    ) -> Result<u64, StreamError> {
        self.within_open(this, |host| {
            let stream = host.table_mut().get(this)?.stream.clone();
            stream.write(&[])?;
            let output = host.table_mut().get_mut(this)?;
            if table::OutputStream::is_flushing(&stream, &output.flushing) {
                output.permit = 0;
                return Ok(0);
            }
            output.flushing.store(false, Ordering::Release);
            output.permit = stream.room();
            Ok(output.permit as u64)
        })
    }

    /// Write what `check-write` permitted; more than it permits traps, as
    /// `io.wit` says a write past the permit must. The permit is for the next
    /// write only, so a write spends all of it; an empty one leaves it, as
    /// wasmtime's does.
    fn streams_output_stream_write(
        &mut self,
        this: OutputStream,
        contents: Vec<u8>,
    ) -> Result<(), StreamError> {
        self.within_open(this, |host| {
            let output = host.table_mut().get_mut(this)?;
            if contents.len() > output.permit {
                return Err(wasmtime::Error::msg(
                    "the guest wrote past what check-write permitted",
                )
                .into());
            }
            if !contents.is_empty() {
                output.permit = 0;
            }
            // A socket whose send buffer shrank since check-write has less
            // room than it permitted; holding the rest would pass the memory
            // the socket reserved, so the write fails whole, before any byte
            // leaves, rather than send the peer a stream with a gap. Room only
            // grows until the write, as the reactor only drains.
            let stream = output.stream.clone();
            if stream.room() < contents.len() {
                return Err(FilesystemError::NoBufferSpace.into());
            }
            stream.write(&contents)?;
            Ok(())
        })
    }

    /// Start a flush, which completes once what was written has left for the
    /// host; `check-write` answers zero until then, as `io.wit` asks.
    fn streams_output_stream_flush(&mut self, this: OutputStream) -> Result<(), StreamError> {
        self.within_open(this, |host| {
            let stream = host.table_mut().get(this)?.stream.clone();
            stream.write(&[])?;
            host.table_mut()
                .get(this)?
                .flushing
                .store(true, Ordering::Release);
            Ok(())
        })
    }

    /// A pollable ready once the stream polls writable and no flush is still
    /// under way, or at once on a closed stream, the stream its parent, as
    /// `io.wit` makes it a child.
    fn streams_output_stream_subscribe(
        &mut self,
        this: OutputStream,
    ) -> wasmtime::Result<Pollable> {
        let output = self.table_mut().get(this)?;
        let pollable = table::Pollable::Output(
            output.stream.clone(),
            output.flushing.clone(),
            output.closed.clone(),
        );
        self.table_mut().push_child(pollable, this)
    }

    fn streams_output_stream_drop(&mut self, handle: OutputStream) -> wasmtime::Result<()> {
        self.table_mut().delete(handle).map(drop)
    }

    /// The failure's POSIX name, so no host text reaches the guest.
    fn error_error_to_debug_string(&mut self, this: Error) -> wasmtime::Result<String> {
        let err = &self.table_mut().get(this)?.0;
        Ok(err.posix_name().unwrap_or("EIO").to_string())
    }

    fn error_error_drop(&mut self, handle: Error) -> wasmtime::Result<()> {
        self.table_mut().delete(handle).map(drop)
    }

    fn poll_pollable_block(&mut self, this: Pollable) -> wasmtime::Result<()> {
        let pollable = self.table_mut().get(this)?.clone();
        self.poll_ready(vec![pollable]).map(drop)
    }

    /// The indices of the pollables ready, once one is, as WASI's `poll`
    /// answers; an empty list traps, as WASI says it does. A handle listed
    /// more than once waits once and answers at each of its indices, as
    /// wasmtime groups the list by handle, so a long list of one handle
    /// costs no more waiters than the handle alone.
    fn poll_poll(&mut self, r#in: Vec<Pollable>) -> wasmtime::Result<Vec<u32>> {
        if r#in.is_empty() {
            return Err(wasmtime::Error::msg("poll was handed no pollable"));
        }
        let mut indices_of = BTreeMap::<u32, Vec<u32>>::new();
        for (index, handle) in (0u32..).zip(&r#in) {
            indices_of.entry(handle.0).or_default().push(index);
        }
        let pollables = indices_of
            .keys()
            .map(|handle| self.table_mut().get(Pollable(*handle)).cloned())
            .collect::<wasmtime::Result<Vec<_>>>()?;
        let ready = self.poll_ready(pollables)?;
        let lists = indices_of.into_values().collect::<Vec<_>>();
        let mut answer = ready
            .into_iter()
            .flat_map(|at| lists[at as usize].iter().copied())
            .collect::<Vec<_>>();
        answer.sort_unstable();
        Ok(answer)
    }

    fn poll_pollable_drop(&mut self, handle: Pollable) -> wasmtime::Result<()> {
        self.table_mut().delete(handle).map(drop)
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
    use super::super::bindings::{Host as _, HttpHost};
    use super::super::error::IntoAnswer;
    use super::*;
    use crate::wasi::{sandbox_connections, Wasi};
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
          (import "wasi:sockets/udp@0.2.12" "[method]outgoing-datagram-stream.send"
            (func $send (param i32 i32 i32 i32)))
          (import "wasi:http/outgoing-handler@0.2.12" "handle"
            (func $handle (param i32 i32 i32 i32)))
          (import "wasi_snapshot_preview1" "sock_accept"
            (func $sock_accept (param i32 i32 i32) (result i32)))
          (import "wasi_snapshot_preview1" "sock_recv"
            (func $sock_recv (param i32 i32 i32 i32 i32 i32) (result i32)))
          (import "wasi_snapshot_preview1" "sock_send"
            (func $sock_send (param i32 i32 i32 i32 i32) (result i32)))
          (import "wasi_snapshot_preview1" "sock_shutdown"
            (func $sock_shutdown (param i32 i32) (result i32)))
          (import "wasi_snapshot_preview1" "fd_fdstat_get"
            (func $fd_fdstat_get (param i32 i32) (result i32)))
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
            (call $resolve (local.get 0) (i32.const 128) (local.get 1) (i32.const 32)))
          (func (export "resolve_into") (param i32 i32)
            (call $resolve (local.get 0) (i32.const 128) (i32.const 0) (local.get 1)))
          (func (export "send") (param i32 i32 i32)
            (call $send (local.get 0) (local.get 1) (local.get 2) (i32.const 32)))
          (func (export "handle") (param i32 i32 i32)
            (call $handle (local.get 0) (local.get 1) (local.get 2) (i32.const 32)))
          (func (export "sock_accept") (param i32) (result i32)
            (call $sock_accept (local.get 0) (i32.const 0) (i32.const 256)))
          (func (export "sock_recv") (param i32) (result i32)
            (call $sock_recv (local.get 0) (i32.const 256) (i32.const 0) (i32.const 0)
              (i32.const 272) (i32.const 276)))
          (func (export "sock_send") (param i32) (result i32)
            (call $sock_send (local.get 0) (i32.const 256) (i32.const 0) (i32.const 0)
              (i32.const 272)))
          (func (export "sock_shutdown") (param i32) (result i32)
            (call $sock_shutdown (local.get 0) (i32.const 3)))
          (func (export "fdstat") (param i32) (result i32)
            (call $fd_fdstat_get (local.get 0) (i32.const 256))))
    "#;

    /// A guest at `profile`, holding at most `open_file_limit` files.
    fn guest(profile: Profile, open_file_limit: Option<usize>) -> (Store<Invocation>, Instance) {
        let config = Config {
            open_file_limit,
            ..Config::for_profile(profile)
        };
        let wasi = Wasi::for_invocation(&config, Vec::new(), &[], &sandbox_connections())
            .expect("WASI context must build");
        instantiate(Invocation::new(None, wasi))
    }

    /// The guest module, instantiated over `invocation`.
    fn instantiate(invocation: Invocation) -> (Store<Invocation>, Instance) {
        let engine = load_engine().expect("shared engine must be constructible");
        let mut store = Store::new(engine, invocation);
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
    fn without_the_network_creating_a_socket_answers_access_denied() {
        let (mut store, instance) = guest(Profile::Hermetic, None);
        call::<(), ()>(&mut store, &instance, "create_ipv4", ()).expect("the call answers");
        let answer = read(&mut store, &instance, 16, 5);
        assert_eq!(
            (answer[0], answer[4]),
            (1, 1),
            "create-tcp-socket with no network entry must answer err(access-denied)"
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
    struct AbandonsCalls;

    impl kobako_runtime::DispatchHandler for AbandonsCalls {
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
        store.data_mut().bind_on_dispatch(Arc::new(AbandonsCalls));
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

    // @behavior NW-163
    #[test]
    fn a_handle_the_guest_does_not_hold_traps_as_it_is_lifted() {
        let (mut store, instance) = guest(Profile::Hermetic, None);
        let mut request = || {
            let guest = store.data_mut();
            let headers = guest.types_fields_new()?;
            let headers = guest.take_handle(headers)?;
            guest
                .types_outgoing_request_new(headers)
                .map(|r| r.0 as i32)
        };
        let (named, unnamed) = (request().unwrap(), request().unwrap());
        let with_options =
            call::<(i32, i32, i32), ()>(&mut store, &instance, "handle", (named, 1, 7))
                .map_err(|trap| trap.root_cause().to_string());
        let without = call::<(i32, i32, i32), ()>(&mut store, &instance, "handle", (unnamed, 0, 0))
            .map_err(|trap| trap.root_cause().to_string());
        assert_eq!(
            (with_options, without),
            (
                Err("the guest named a handle it does not hold".to_string()),
                Ok(())
            ),
            "outgoing-handler.handle naming request-options the guest was never handed must trap \
             as the handle is lifted, as wasmtime's, and a request with no options must be answered"
        );
    }

    // @behavior RT-374
    #[test]
    fn lists_past_the_calls_hostcall_fuel_trap_before_the_host_answers() {
        let peer = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let port = peer.local_addr().unwrap().port();
        let (mut store, instance) = instantiate(network_guest(
            network::Endpoint::new("127.0.0.1", port)
                .unwrap()
                .with_protocol(network::Protocol::Udp),
        ));
        let outgoing = run_task(Some(Instant::now() + Duration::from_secs(10)), || {
            let guest = store.data_mut();
            let network = guest.instance_network_instance_network()?;
            let socket = guest
                .udp_create_socket_create_udp_socket(IpAddressFamily::Ipv4)
                .into_answer()?
                .expect("a network makes a socket");
            guest
                .udp_udp_socket_start_bind(socket, network, ipv4((0, 0, 0, 0), 0))
                .into_answer()?
                .expect("an ephemeral bind is the implicit one");
            guest
                .udp_udp_socket_finish_bind(socket)
                .into_answer()?
                .expect("the bind settles");
            let (_, outgoing) = guest
                .udp_udp_socket_stream(socket, Some(ipv4((127, 0, 0, 1), port)))
                .into_answer()?
                .expect("the udp endpoint names the peer");
            Ok::<_, wasmtime::Error>(outgoing)
        })
        .expect("the stream is made");
        let memory = instance.get_memory(&mut store, "memory").expect("memory");
        memory.grow(&mut store, 3).expect("four pages");
        // Each datagram's data names the same 60000 bytes at 0, so 2400 of
        // them name 144 MB while the guest holds 256 KiB.
        const RECORDS_AT: usize = 65536;
        for i in 0..2400 {
            let record = RECORDS_AT + i * 44;
            memory.data_mut(&mut store)[record + 4..record + 8]
                .copy_from_slice(&60_000u32.to_le_bytes());
        }
        // No send was permitted, so a call the lift lets through traps there.
        let stream = outgoing.0 as i32;
        let answer = |store: &mut Store<Invocation>, count: i32| {
            call::<(i32, i32, i32), ()>(
                store,
                &instance,
                "send",
                (stream, RECORDS_AT as i32, count),
            )
            .map_err(|trap| trap.root_cause().to_string())
        };
        let past = answer(&mut store, 2400);
        let within = answer(&mut store, 2);
        assert!(
            past.as_ref()
                .is_err_and(|trap| trap.contains("hostcall fuel"))
                && within
                    .as_ref()
                    .is_err_and(|trap| trap.contains("more datagrams than check-send permitted")),
            "outgoing-datagram-stream.send of 2400 datagrams naming one 60000-byte region must \
             trap on its hostcall fuel before the host answers, and of two must reach the host; \
             got {past:?} and {within:?}"
        );
    }

    // @behavior RT-373
    #[test]
    fn an_answer_into_a_misaligned_return_area_traps() {
        let (mut store, instance) = guest(Profile::Hermetic, None);
        let network = call::<(), i32>(&mut store, &instance, "network", ()).expect("a handle");
        let misaligned =
            call::<(i32, i32), ()>(&mut store, &instance, "resolve_into", (network, 33));
        let aligned = call::<(i32, i32), ()>(&mut store, &instance, "resolve_into", (network, 32));
        assert!(
            misaligned.is_err() && aligned.is_ok(),
            "resolve-addresses answering into a return area off its 4-byte alignment must trap, \
             as the Canonical ABI does, and answer into an aligned one"
        );
    }

    // @behavior RT-338
    #[test]
    fn a_clock_wait_on_the_hermetic_profile_ends_at_once() {
        let (mut store, instance) = guest(Profile::Hermetic, None);
        let started = Instant::now();
        let pollable = call::<i64, i32>(&mut store, &instance, "subscribe", 10_000_000_000)
            .expect("a pollable");
        call::<i32, ()>(&mut store, &instance, "block", pollable).expect("the wait answers");
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "pollable.block on ten seconds of subscribe-duration on the hermetic profile must \
             return at once, not after {:?}",
            started.elapsed()
        );
    }

    // @behavior RT-371
    #[test]
    fn a_hermetic_poll_on_two_clocks_finds_both_ready() {
        let (mut store, _) = guest(Profile::Hermetic, None);
        let guest = store.data_mut();
        let clocks = [1_000_000_000, 2_000_000_000]
            .map(|nanos| guest.monotonic_clock_subscribe_duration(nanos).unwrap());
        let ready = guest.poll_poll(clocks.to_vec()).unwrap();
        assert_eq!(
            ready,
            [0, 1],
            "poll on a one- and a two-second subscribe-duration at the hermetic profile must \
             find both ready at once"
        );
    }

    // @behavior RT-387
    #[test]
    fn a_pollable_listed_twice_answers_at_each_index() {
        let (mut store, _) = guest(Profile::Permissive, None);
        let guest = store.data_mut();
        let far = guest
            .monotonic_clock_subscribe_duration(60_000_000_000)
            .unwrap();
        let near = guest.monotonic_clock_subscribe_duration(1_000_000).unwrap();
        let ready = guest.poll_poll(vec![far, near, far, near]).unwrap();
        assert_eq!(
            ready,
            [1, 3],
            "poll on a list naming a far and a near clock twice each must answer both indices of \
             the near clock"
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
            "poll of one pollable on fifty milliseconds of subscribe-duration on the permissive \
             profile must answer [0] once they pass, got {len} index {index} after {spent:?}"
        );
    }

    /// A hermetic invocation whose network reaches `port` on loopback over TCP.
    fn loopback_guest(port: u16) -> Invocation {
        network_guest(
            network::Endpoint::new("127.0.0.1", port)
                .unwrap()
                .with_protocol(network::Protocol::Tcp),
        )
    }

    /// A hermetic invocation whose network holds `endpoint`, or none.
    fn network_guest(endpoint: network::Endpoint) -> Invocation {
        limited_network_guest([endpoint], None)
    }

    /// A hermetic invocation whose network holds `endpoints`, holding at most
    /// `open_file_limit` files.
    fn limited_network_guest(
        endpoints: impl IntoIterator<Item = network::Endpoint>,
        open_file_limit: Option<usize>,
    ) -> Invocation {
        Invocation::hermetic(Some(endpoints.into_iter().collect()), |config| {
            config.open_file_limit = open_file_limit
        })
    }

    // @behavior NW-030
    #[test]
    fn without_the_network_a_lookup_answers_permanent_resolver_failure() {
        let (mut store, _instance) = guest(Profile::Hermetic, None);
        let guest = store.data_mut();
        let network = guest.instance_network_instance_network().unwrap();
        assert!(
            matches!(
                guest
                    .ip_name_lookup_resolve_addresses(network, "example.com".to_string())
                    .into_answer(),
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
        let mut guest = network_guest(
            network::Endpoint::new("localhost", port)
                .unwrap()
                .with_protocol(network::Protocol::Tcp)
                .with_addresses(vec![network::Addresses::Local]),
        );
        let started = Instant::now();
        let (handed, end, remote) = run_task(Some(started + Duration::from_secs(10)), || {
            let network = guest.instance_network_instance_network()?;
            let stream = guest
                .ip_name_lookup_resolve_addresses(network, "localhost".to_string())
                .into_answer()?
                .expect("an endpoint names the name");
            let answered = guest.ip_name_lookup_resolve_address_stream_subscribe(stream)?;
            guest.poll_pollable_block(answered)?;
            let handed = guest
                .ip_name_lookup_resolve_address_stream_resolve_next_address(stream)
                .into_answer()?
                .expect("localhost resolves");
            let end = guest
                .ip_name_lookup_resolve_address_stream_resolve_next_address(stream)
                .into_answer()?;
            let Some(IpAddress::Ipv4(address)) = handed else {
                return Err(wasmtime::Error::msg("no IPv4 address handed out"));
            };
            let socket = guest
                .tcp_create_socket_create_tcp_socket(IpAddressFamily::Ipv4)
                .into_answer()?
                .expect("a network makes a socket");
            let remote = IpSocketAddress::Ipv4(Ipv4SocketAddress { port, address });
            guest
                .tcp_tcp_socket_start_connect(socket, network, remote)
                .into_answer()?
                .expect("the endpoint leads to loopback");
            let connecting = guest.tcp_tcp_socket_subscribe(socket)?;
            guest.poll_pollable_block(connecting)?;
            guest
                .tcp_tcp_socket_finish_connect(socket)
                .into_answer()?
                .expect("the listener accepts");
            Ok((
                handed,
                end,
                guest.tcp_tcp_socket_remote_address(socket).into_answer()?,
            ))
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

    // @behavior NW-113
    #[test]
    fn a_socket_dropped_while_its_streams_live_traps() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let peer = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let (tcp_port, udp_port) = (
            listener.local_addr().unwrap().port(),
            peer.local_addr().unwrap().port(),
        );
        let mut guest = limited_network_guest(
            [
                network::Endpoint::new("127.0.0.1", tcp_port)
                    .unwrap()
                    .with_protocol(network::Protocol::Tcp),
                network::Endpoint::new("127.0.0.1", udp_port)
                    .unwrap()
                    .with_protocol(network::Protocol::Udp),
            ],
            None,
        );
        let at = |port| {
            IpSocketAddress::Ipv4(Ipv4SocketAddress {
                port,
                address: (127, 0, 0, 1),
            })
        };
        let answers = run_task(Some(Instant::now() + Duration::from_secs(10)), || {
            let network = guest.instance_network_instance_network()?;
            let tcp = guest
                .tcp_create_socket_create_tcp_socket(IpAddressFamily::Ipv4)
                .into_answer()?
                .expect("a network makes a socket");
            guest
                .tcp_tcp_socket_start_connect(tcp, network, at(tcp_port))
                .into_answer()?
                .expect("an endpoint names the address");
            let connecting = guest.tcp_tcp_socket_subscribe(tcp)?;
            guest.poll_pollable_block(connecting)?;
            guest.poll_pollable_drop(connecting)?;
            let (input, output) = guest
                .tcp_tcp_socket_finish_connect(tcp)
                .into_answer()?
                .expect("the listener accepts");
            let streams_held = guest.tcp_tcp_socket_drop(tcp).is_err();
            guest.streams_input_stream_drop(input)?;
            guest.streams_output_stream_drop(output)?;
            let ready = guest.tcp_tcp_socket_subscribe(tcp)?;
            let pollable_held = guest.tcp_tcp_socket_drop(tcp).is_err();
            guest.poll_pollable_drop(ready)?;
            let tcp_late = guest.tcp_tcp_socket_drop(tcp).is_ok();

            let udp = guest
                .udp_create_socket_create_udp_socket(IpAddressFamily::Ipv4)
                .into_answer()?
                .expect("a network makes a socket");
            let any = IpSocketAddress::Ipv4(Ipv4SocketAddress {
                port: 0,
                address: (0, 0, 0, 0),
            });
            guest
                .udp_udp_socket_start_bind(udp, network, any)
                .into_answer()?
                .expect("an ephemeral bind is the implicit one");
            guest
                .udp_udp_socket_finish_bind(udp)
                .into_answer()?
                .expect("the bind settles");
            let (incoming, outgoing) = guest
                .udp_udp_socket_stream(udp, Some(at(udp_port)))
                .into_answer()?
                .expect("the udp endpoint names the peer");
            let again_early = guest
                .udp_udp_socket_stream(udp, Some(at(udp_port)))
                .into_answer()
                .is_err();
            let udp_early = guest.udp_udp_socket_drop(udp).is_err();
            guest.udp_incoming_datagram_stream_drop(incoming)?;
            guest.udp_outgoing_datagram_stream_drop(outgoing)?;
            let again_late = guest
                .udp_udp_socket_stream(udp, Some(at(udp_port)))
                .into_answer()?
                .map(|(incoming, outgoing)| {
                    guest.udp_incoming_datagram_stream_drop(incoming)?;
                    guest.udp_outgoing_datagram_stream_drop(outgoing)
                })
                .is_ok_and(|dropped| dropped.is_ok());
            let udp_late = guest.udp_udp_socket_drop(udp).is_ok();
            Ok::<_, wasmtime::Error>([
                streams_held,
                pollable_held,
                tcp_late,
                again_early,
                udp_early,
                again_late,
                udp_late,
            ])
        })
        .expect("the sockets connect");
        assert_eq!(
            answers, [true; 7],
            "a tcp-socket or udp-socket dropped while its streams or pollable live, and a second \
             udp stream while the first lives, must trap as wasmtime-wasi traps them, and each \
             must succeed once the children are dropped"
        );
    }

    // @behavior NW-153
    #[test]
    fn a_udp_socket_answers_bound_and_empty_sends_as_wasmtimes() {
        let peer = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let port = peer.local_addr().unwrap().port();
        let mut guest = network_guest(
            network::Endpoint::new("127.0.0.1", port)
                .unwrap()
                .with_protocol(network::Protocol::Udp),
        );
        let answers = (|| {
            let network = guest.instance_network_instance_network()?;
            let udp = guest
                .udp_create_socket_create_udp_socket(IpAddressFamily::Ipv4)
                .into_answer()?
                .expect("a network makes a socket");
            guest
                .udp_udp_socket_start_bind(udp, network, ipv4((0, 0, 0, 0), 0))
                .into_answer()?
                .expect("an ephemeral bind is the implicit one");
            let binding = guest
                .udp_udp_socket_local_address(udp)
                .into_answer()?
                .is_ok();
            guest
                .udp_udp_socket_finish_bind(udp)
                .into_answer()?
                .expect("the bind settles");
            let (_, outgoing) = guest
                .udp_udp_socket_stream(udp, Some(ipv4((127, 0, 0, 1), port)))
                .into_answer()?
                .expect("the udp endpoint names the peer");
            let permitted = guest
                .udp_outgoing_datagram_stream_check_send(outgoing)
                .into_answer()?
                .expect("a streaming socket permits");
            let empty = guest
                .udp_outgoing_datagram_stream_send(outgoing, Vec::new())
                .into_answer()?
                .ok();
            let kept = guest
                .udp_outgoing_datagram_stream_send(
                    outgoing,
                    vec![OutgoingDatagram {
                        data: b"x".to_vec(),
                        remote_address: None,
                    }],
                )
                .into_answer()?
                .ok();
            Ok::<_, wasmtime::Error>((binding, permitted > 0, empty, kept))
        })()
        .expect("the socket streams");
        assert_eq!(
            answers,
            (true, true, Some(0), Some(1)),
            "a UDP socket must answer local-address once its bind began, and an empty send must \
             answer 0 without spending check-send's permit, as wasmtime-wasi answers both"
        );
    }

    // @behavior NW-147
    #[test]
    fn a_producer_dropped_while_its_pollable_lives_traps() {
        use super::super::bindings::wasi_http_types_0_2_12::Scheme;
        use super::super::bindings::HttpHost;
        let peer = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let udp_port = peer.local_addr().unwrap().port();
        let mut guest = limited_network_guest(
            [
                network::Endpoint::new("127.0.0.1", udp_port)
                    .unwrap()
                    .with_protocol(network::Protocol::Udp),
                network::Endpoint::new("localhost", 80)
                    .unwrap()
                    .with_addresses(vec![network::Addresses::Local]),
            ],
            None,
        );
        let answers = (|| {
            let network = guest.instance_network_instance_network()?;
            let udp = guest
                .udp_create_socket_create_udp_socket(IpAddressFamily::Ipv4)
                .into_answer()?
                .expect("a network makes a socket");
            let any = IpSocketAddress::Ipv4(Ipv4SocketAddress {
                port: 0,
                address: (0, 0, 0, 0),
            });
            guest
                .udp_udp_socket_start_bind(udp, network, any)
                .into_answer()?
                .expect("an ephemeral bind is the implicit one");
            guest
                .udp_udp_socket_finish_bind(udp)
                .into_answer()?
                .expect("the bind settles");
            let peer_at = IpSocketAddress::Ipv4(Ipv4SocketAddress {
                port: udp_port,
                address: (127, 0, 0, 1),
            });
            let (incoming, outgoing) = guest
                .udp_udp_socket_stream(udp, Some(peer_at))
                .into_answer()?
                .expect("the udp endpoint names the peer");
            let readable = guest.udp_incoming_datagram_stream_subscribe(incoming)?;
            let writable = guest.udp_outgoing_datagram_stream_subscribe(outgoing)?;
            let datagrams_held = guest.udp_incoming_datagram_stream_drop(incoming).is_err()
                && guest.udp_outgoing_datagram_stream_drop(outgoing).is_err();
            guest.poll_pollable_drop(readable)?;
            guest.poll_pollable_drop(writable)?;
            let datagrams_late = guest.udp_incoming_datagram_stream_drop(incoming).is_ok()
                && guest.udp_outgoing_datagram_stream_drop(outgoing).is_ok();

            let lookup = guest
                .ip_name_lookup_resolve_addresses(network, "localhost".to_string())
                .into_answer()?
                .expect("an endpoint names localhost");
            let resolved = guest.ip_name_lookup_resolve_address_stream_subscribe(lookup)?;
            let lookup_held = guest
                .ip_name_lookup_resolve_address_stream_drop(lookup)
                .is_err();
            guest.poll_pollable_drop(resolved)?;
            let lookup_late = guest
                .ip_name_lookup_resolve_address_stream_drop(lookup)
                .is_ok();

            let headers = guest.types_fields_new()?;
            let headers = guest.take_handle(headers)?;
            let request = guest.types_outgoing_request_new(headers)?;
            guest
                .types_outgoing_request_set_scheme(request, Some(Scheme::HTTP))?
                .expect("http is a scheme");
            guest
                .types_outgoing_request_set_authority(request, Some("other.test".to_string()))?
                .expect("an authority");
            let body = guest
                .types_outgoing_request_body(request)?
                .expect("a request hands out its body once");
            let request = guest.take_handle(request)?;
            let future = guest
                .outgoing_handler_handle(request, None)
                .into_answer()?
                .expect("a request whose body is unfinished waits in its future");
            guest.types_outgoing_body_drop(body)?;
            let arrived = guest.types_future_incoming_response_subscribe(future)?;
            let future_held = guest.types_future_incoming_response_drop(future).is_err();
            guest.poll_pollable_drop(arrived)?;
            let future_late = guest.types_future_incoming_response_drop(future).is_ok();
            Ok::<_, wasmtime::Error>([
                datagrams_held,
                datagrams_late,
                lookup_held,
                lookup_late,
                future_held,
                future_late,
            ])
        })()
        .expect("the producers are made");
        assert_eq!(
            answers, [true; 6],
            "a datagram stream, a resolve-address-stream, or a future-incoming-response dropped \
             while its pollable lives must trap, as wasmtime-wasi makes every pollable a child, \
             and each must drop once its pollable is gone"
        );
    }

    // @behavior NW-114
    #[test]
    fn a_tcp_socket_reads_its_own_end_only_once_connected() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let open = listener.local_addr().unwrap().port();
        let closed = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let tcp = |port| {
            network::Endpoint::new("127.0.0.1", port)
                .unwrap()
                .with_protocol(network::Protocol::Tcp)
        };
        let mut guest = limited_network_guest([tcp(open), tcp(closed)], None);
        let (during, failure, after) =
            run_task(Some(Instant::now() + Duration::from_secs(10)), || {
                let network = guest.instance_network_instance_network()?;
                let connecting = guest
                    .tcp_create_socket_create_tcp_socket(IpAddressFamily::Ipv4)
                    .into_answer()?
                    .expect("a network makes a socket");
                guest
                    .tcp_tcp_socket_start_connect(connecting, network, ipv4((127, 0, 0, 1), open))
                    .into_answer()?
                    .expect("an endpoint names the listener");
                let during = guest
                    .tcp_tcp_socket_local_address(connecting)
                    .into_answer()?;
                let refused = guest
                    .tcp_create_socket_create_tcp_socket(IpAddressFamily::Ipv4)
                    .into_answer()?
                    .expect("a network makes a socket");
                guest
                    .tcp_tcp_socket_start_connect(refused, network, ipv4((127, 0, 0, 1), closed))
                    .into_answer()?
                    .expect("an endpoint names the closed port");
                let settled = guest.tcp_tcp_socket_subscribe(refused)?;
                guest.poll_pollable_block(settled)?;
                let failure = guest
                    .tcp_tcp_socket_finish_connect(refused)
                    .into_answer()?
                    .err();
                let after = (
                    guest.tcp_tcp_socket_local_address(refused).into_answer()?,
                    guest
                        .tcp_tcp_socket_remote_address(refused)
                        .into_answer()?
                        .err(),
                );
                Ok::<_, wasmtime::Error>((during, failure, after))
            })
            .expect("the connects answer");
        assert_eq!(
            (during, failure, after),
            (
                Err(ErrorCode::InvalidState),
                Some(ErrorCode::ConnectionRefused),
                (
                    Err(ErrorCode::ConnectionRefused),
                    Some(ErrorCode::ConnectionRefused)
                )
            ),
            "local-address of a TCP socket still connecting must answer invalid-state, and \
             local-address and remote-address of one whose connect was refused the connect's \
             error, as wasmtime-wasi answers each"
        );
    }

    // @behavior NW-167
    #[test]
    fn a_tcp_socket_whose_connect_failed_connects_no_more() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut guest = network_guest(network::Endpoint::new("127.0.0.1", port).unwrap());
        let answers = run_task(Some(Instant::now() + Duration::from_secs(10)), || {
            let network = guest.instance_network_instance_network()?;
            let socket = guest
                .tcp_create_socket_create_tcp_socket(IpAddressFamily::Ipv4)
                .into_answer()?
                .expect("a network makes a socket");
            let started = guest
                .tcp_tcp_socket_start_connect(socket, network, ipv4((127, 0, 0, 1), port ^ 1))
                .into_answer()?;
            let refused = guest
                .tcp_tcp_socket_finish_connect(socket)
                .into_answer()?
                .map(drop);
            let again = guest
                .tcp_tcp_socket_start_connect(socket, network, ipv4((127, 0, 0, 1), port))
                .into_answer()?;
            Ok::<_, wasmtime::Error>((started, refused, again))
        })
        .expect("the calls answer");
        assert_eq!(
            answers,
            (
                Ok(()),
                Err(ErrorCode::AccessDenied),
                Err(ErrorCode::InvalidState)
            ),
            "a TCP socket's start-connect no endpoint allows must succeed and its finish-connect \
             answer access-denied, as wasmtime-wasi's connect fails once under way, and a later \
             connect it would allow invalid-state, as sockets.wit closes a socket after a failed \
             connection attempt"
        );
    }

    // @behavior NW-166
    #[test]
    fn a_udp_stream_leaves_a_bind_under_way_standing() {
        let peer = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let port = peer.local_addr().unwrap().port();
        let mut guest = network_guest(
            network::Endpoint::new("127.0.0.1", port)
                .unwrap()
                .with_protocol(network::Protocol::Udp),
        );
        let answers = run_task(Some(Instant::now() + Duration::from_secs(10)), || {
            let network = guest.instance_network_instance_network()?;
            let socket = guest
                .udp_create_socket_create_udp_socket(IpAddressFamily::Ipv4)
                .into_answer()?
                .expect("a network makes a socket");
            guest
                .udp_udp_socket_start_bind(socket, network, ipv4((0, 0, 0, 0), 0))
                .into_answer()?
                .expect("an ephemeral bind is the implicit one");
            guest
                .udp_udp_socket_stream(socket, Some(ipv4((127, 0, 0, 1), port)))
                .into_answer()?
                .expect("a socket whose bind began streams");
            let again = guest
                .udp_udp_socket_start_bind(socket, network, ipv4((0, 0, 0, 0), 0))
                .into_answer()?;
            let finished = guest.udp_udp_socket_finish_bind(socket).into_answer()?;
            Ok::<_, wasmtime::Error>((again, finished))
        })
        .expect("the calls answer");
        assert_eq!(
            answers,
            (Err(ErrorCode::ConcurrencyConflict), Ok(())),
            "a UDP socket streaming between start-bind and finish-bind must answer another \
             start-bind concurrency-conflict and let finish-bind settle, as wasmtime keeps its \
             bind under way apart from its streams"
        );
    }

    // @behavior NW-115
    #[test]
    fn one_datagram_receive_answers_at_most_sixteen() {
        let peer = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let port = peer.local_addr().unwrap().port();
        let mut guest = network_guest(
            network::Endpoint::new("127.0.0.1", port)
                .unwrap()
                .with_protocol(network::Protocol::Udp),
        );
        let batches = run_task(Some(Instant::now() + Duration::from_secs(10)), || {
            let network = guest.instance_network_instance_network()?;
            let socket = guest
                .udp_create_socket_create_udp_socket(IpAddressFamily::Ipv4)
                .into_answer()?
                .expect("a network makes a socket");
            guest
                .udp_udp_socket_start_bind(socket, network, ipv4((0, 0, 0, 0), 0))
                .into_answer()?
                .expect("an ephemeral bind is the implicit one");
            guest
                .udp_udp_socket_finish_bind(socket)
                .into_answer()?
                .expect("the bind settles");
            let (incoming, outgoing) = guest
                .udp_udp_socket_stream(socket, Some(ipv4((127, 0, 0, 1), port)))
                .into_answer()?
                .expect("the udp endpoint names the peer");
            let room = guest.udp_outgoing_datagram_stream_subscribe(outgoing)?;
            guest.poll_pollable_block(room)?;
            guest
                .udp_outgoing_datagram_stream_check_send(outgoing)
                .into_answer()?
                .expect("the stream with room permits a datagram");
            guest
                .udp_outgoing_datagram_stream_send(
                    outgoing,
                    vec![OutgoingDatagram {
                        data: b"hi".to_vec(),
                        remote_address: None,
                    }],
                )
                .into_answer()?
                .expect("the datagram is sent");
            let mut buf = [0; 16];
            let (_, guest_end) = peer.recv_from(&mut buf)?;
            for _ in 0..20 {
                peer.send_to(b"ping", guest_end)?;
            }
            std::thread::sleep(Duration::from_millis(500));
            let mut batches = Vec::new();
            while batches.iter().sum::<usize>() < 20 {
                let arrived = guest.udp_incoming_datagram_stream_subscribe(incoming)?;
                guest.poll_pollable_block(arrived)?;
                guest.poll_pollable_drop(arrived)?;
                batches.push(
                    guest
                        .udp_incoming_datagram_stream_receive(incoming, 100)
                        .into_answer()?
                        .expect("the peer's datagrams are received")
                        .len(),
                );
            }
            Ok::<_, wasmtime::Error>(batches)
        })
        .expect("the datagrams arrive");
        assert_eq!(
            batches,
            [16, 4],
            "receive with max-results 100 of twenty queued datagrams must answer sixteen, then the \
             other four, as wasmtime-wasi bounds one receive"
        );
    }

    // @behavior NW-055
    #[test]
    fn a_udp_socket_streams_only_with_a_peer_it_names() {
        let peer = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let port = peer.local_addr().unwrap().port();
        let mut guest = network_guest(
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
                .udp_create_socket_create_udp_socket(IpAddressFamily::Ipv4)
                .into_answer()?
                .expect("a network makes a socket");
            let fixed = guest
                .udp_udp_socket_start_bind(fixed_socket, network, any(4000))
                .into_answer()?;
            let socket = guest
                .udp_create_socket_create_udp_socket(IpAddressFamily::Ipv4)
                .into_answer()?
                .expect("a network makes a socket");
            guest
                .udp_udp_socket_start_bind(socket, network, any(0))
                .into_answer()?
                .expect("an ephemeral bind is the implicit one");
            guest
                .udp_udp_socket_finish_bind(socket)
                .into_answer()?
                .expect("the bind settles");
            let peerless = guest
                .udp_udp_socket_stream(socket, None)
                .into_answer()?
                .map(drop);
            let (incoming, outgoing) = guest
                .udp_udp_socket_stream(socket, Some(at(port)))
                .into_answer()?
                .expect("the udp endpoint names the peer");
            let room = guest.udp_outgoing_datagram_stream_subscribe(outgoing)?;
            guest.poll_pollable_block(room)?;
            guest
                .udp_outgoing_datagram_stream_check_send(outgoing)
                .into_answer()?
                .expect("the stream with room permits a datagram");
            let sent = guest.udp_outgoing_datagram_stream_send(
                outgoing,
                vec![OutgoingDatagram {
                    data: b"ping".to_vec(),
                    remote_address: None,
                }],
            );
            if !matches!(sent, Ok(1)) {
                return Err(wasmtime::Error::msg("the datagram was not sent"));
            }
            let mut buf = [0; 16];
            let (n, from) = peer.recv_from(&mut buf)?;
            peer.send_to(&buf[..n].to_ascii_uppercase(), from)?;
            let arrived = guest.udp_incoming_datagram_stream_subscribe(incoming)?;
            guest.poll_pollable_block(arrived)?;
            let answer = guest
                .udp_incoming_datagram_stream_receive(incoming, 4)
                .into_answer()?
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

    fn ipv4(address: (u8, u8, u8, u8), port: u16) -> IpSocketAddress {
        IpSocketAddress::Ipv4(Ipv4SocketAddress { port, address })
    }

    /// A guest whose network reaches a loopback listener over TCP and UDP,
    /// and the listener's port.
    fn guest_and_listener() -> (Invocation, std::net::TcpListener) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let endpoint = network::Endpoint::new("127.0.0.1", port)
            .unwrap()
            .with_protocol(network::Protocol::Tcp)
            .with_addresses(vec![network::Addresses::Local]);
        (network_guest(endpoint), listener)
    }

    // @behavior NW-165
    #[test]
    fn a_read_of_nothing_after_a_failed_read_answers_closed() {
        let (mut guest, listener) = guest_and_listener();
        let port = listener.local_addr().unwrap().port();
        let answers = run_task(Some(Instant::now() + Duration::from_secs(10)), || {
            let network = guest.instance_network_instance_network()?;
            let socket = guest
                .tcp_create_socket_create_tcp_socket(IpAddressFamily::Ipv4)
                .into_answer()?
                .expect("a network makes a socket");
            guest
                .tcp_tcp_socket_start_connect(socket, network, ipv4((127, 0, 0, 1), port))
                .into_answer()?
                .expect("the endpoint names the listener");
            let connecting = guest.tcp_tcp_socket_subscribe(socket)?;
            guest.poll_pollable_block(connecting)?;
            let (input, output) = guest
                .tcp_tcp_socket_finish_connect(socket)
                .into_answer()?
                .expect("the listener accepts");
            let (peer, _) = listener.accept().unwrap();
            // The peer closes with these bytes unread, so it resets.
            guest
                .streams_output_stream_blocking_write_and_flush(output, b"unread".to_vec())
                .into_answer()?
                .expect("the bytes leave");
            std::thread::sleep(Duration::from_millis(20));
            drop(peer);
            let failed = loop {
                let readable = guest.streams_input_stream_subscribe(input)?;
                guest.poll_pollable_block(readable)?;
                guest.poll_pollable_drop(readable)?;
                match guest.streams_input_stream_read(input, 8).into_answer()? {
                    Ok(bytes) if bytes.is_empty() => continue,
                    answer => break answer.is_err(),
                }
            };
            let nothing = matches!(
                guest.streams_input_stream_read(input, 0).into_answer()?,
                Err(crate::preview2::error::StreamError::Closed)
            );
            Ok::<_, wasmtime::Error>((failed, nothing))
        })
        .expect("the calls answer");
        assert_eq!(
            answers,
            (true, true),
            "input-stream.read of nothing after a read failed on a reset must answer closed, as \
             wasmtime's stream closes on a failed read"
        );
    }

    // @behavior NW-155
    #[test]
    fn a_flushing_streams_pollable_waits_until_what_it_flushes_has_left() {
        let (mut guest, listener) = guest_and_listener();
        let port = listener.local_addr().unwrap().port();
        let started = Instant::now();
        let answers = run_task(Some(started + Duration::from_secs(30)), || {
            let network = guest.instance_network_instance_network()?;
            let socket = guest
                .tcp_create_socket_create_tcp_socket(IpAddressFamily::Ipv4)
                .into_answer()?
                .expect("a network makes a socket");
            guest
                .tcp_tcp_socket_set_send_buffer_size(socket, 4096)
                .into_answer()?
                .expect("a socket takes a send buffer");
            guest
                .tcp_tcp_socket_start_connect(socket, network, ipv4((127, 0, 0, 1), port))
                .into_answer()?
                .expect("the endpoint names the listener");
            let connecting = guest.tcp_tcp_socket_subscribe(socket)?;
            guest.poll_pollable_block(connecting)?;
            let (_, output) = guest
                .tcp_tcp_socket_finish_connect(socket)
                .into_answer()?
                .expect("the listener accepts");
            let (mut peer, _) = listener.accept().unwrap();
            // Fill the host's buffers and the socket's own, the peer reading
            // nothing, so what a flush holds cannot leave.
            let mut quiet = 0;
            while quiet < 20 {
                match guest
                    .streams_output_stream_check_write(output)
                    .into_answer()?
                {
                    Ok(0) => {
                        quiet += 1;
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Ok(room) => {
                        quiet = 0;
                        let chunk = vec![b'x'; room.min(4096) as usize];
                        let _ = guest
                            .streams_output_stream_write(output, chunk)
                            .into_answer()?;
                    }
                    Err(_) => break,
                }
            }
            guest
                .streams_output_stream_flush(output)
                .into_answer()?
                .expect("a connected stream flushes");
            let writable = guest.streams_output_stream_subscribe(output)?;
            let now = guest.monotonic_clock_subscribe_duration(0)?;
            let while_held = guest.poll_poll(vec![writable, now])?;
            let reader = std::thread::spawn(move || {
                let mut sink = [0u8; 65536];
                while std::io::Read::read(&mut peer, &mut sink).is_ok_and(|n| n > 0) {}
            });
            guest.poll_pollable_block(writable)?;
            let after = guest
                .streams_output_stream_check_write(output)
                .into_answer()?
                .ok();
            Ok::<_, wasmtime::Error>((while_held, after, reader))
        })
        .expect("the stream fills and drains");
        let (while_held, after, _reader) = answers;
        assert!(
            while_held == [1] && after.is_some_and(|room| room > 0),
            "output-stream.subscribe after a flush the peer has not taken must not be ready, \
             only the zero-length clock beside it, and once the peer reads, it must be ready \
             with check-write offering bytes; polled {while_held:?}, then offered {after:?}"
        );
    }

    // @behavior NW-089
    #[test]
    fn an_address_wasi_calls_invalid_answers_invalid_argument() {
        let (mut guest, _listener) = guest_and_listener();
        let network = guest.instance_network_instance_network().unwrap();
        let connect = |guest: &mut Invocation, to| {
            let socket = guest
                .tcp_create_socket_create_tcp_socket(IpAddressFamily::Ipv4)
                .into_answer()
                .unwrap()
                .expect("a network makes a socket");
            guest
                .tcp_tcp_socket_start_connect(socket, network, to)
                .into_answer()
                .unwrap()
        };
        let connects = [
            connect(&mut guest, ipv4((0, 0, 0, 0), 80)),
            connect(&mut guest, ipv4((127, 0, 0, 1), 0)),
            connect(&mut guest, ipv4((255, 255, 255, 255), 80)),
            connect(&mut guest, ipv4((224, 0, 0, 1), 80)),
        ];
        let udp = guest
            .udp_create_socket_create_udp_socket(IpAddressFamily::Ipv4)
            .into_answer()
            .unwrap()
            .expect("a network makes a socket");
        let tcp_bind = {
            let socket = guest
                .tcp_create_socket_create_tcp_socket(IpAddressFamily::Ipv4)
                .into_answer()
                .unwrap()
                .expect("a network makes a socket");
            guest
                .tcp_tcp_socket_start_bind(socket, network, ipv4((224, 0, 0, 1), 0))
                .into_answer()
                .unwrap()
        };
        let udp_bind = guest
            .udp_udp_socket_start_bind(udp, network, ipv4((224, 0, 0, 1), 0))
            .into_answer()
            .unwrap();
        let bound = guest
            .udp_create_socket_create_udp_socket(IpAddressFamily::Ipv4)
            .into_answer()
            .unwrap()
            .expect("a network makes a socket");
        guest
            .udp_udp_socket_start_bind(bound, network, ipv4((0, 0, 0, 0), 0))
            .into_answer()
            .unwrap()
            .expect("an ephemeral bind is the implicit one");
        guest
            .udp_udp_socket_finish_bind(bound)
            .into_answer()
            .unwrap()
            .expect("the bind settles");
        let mut stream = |to| {
            guest
                .udp_udp_socket_stream(bound, Some(to))
                .into_answer()
                .unwrap()
                .map(drop)
        };
        let streams = [
            stream(ipv4((0, 0, 0, 0), 80)),
            stream(ipv4((127, 0, 0, 1), 0)),
            stream(ipv4((255, 255, 255, 255), 80)),
            stream(ipv4((224, 0, 0, 1), 80)),
        ];
        assert_eq!(
            (connects, tcp_bind, udp_bind, streams),
            (
                [Err(ErrorCode::InvalidArgument); 4],
                Err(ErrorCode::InvalidArgument),
                Err(ErrorCode::AccessDenied),
                [
                    Err(ErrorCode::InvalidArgument),
                    Err(ErrorCode::InvalidArgument),
                    Err(ErrorCode::AccessDenied),
                    Err(ErrorCode::AccessDenied)
                ]
            ),
            "start-connect to 0.0.0.0, port 0, a broadcast, or a multicast address, and a TCP \
             start-bind to a multicast address, must answer invalid-argument; a UDP socket \
             checks only the unspecified address and port 0 that way, as wasmtime's, so its \
             multicast bind and broadcast or multicast stream answer access-denied"
        );
    }

    // @behavior NW-090
    #[test]
    fn a_second_start_while_one_is_under_way_answers_concurrency_conflict() {
        let (mut guest, listener) = guest_and_listener();
        let port = listener.local_addr().unwrap().port();
        let network = guest.instance_network_instance_network().unwrap();
        let socket = guest
            .tcp_create_socket_create_tcp_socket(IpAddressFamily::Ipv4)
            .into_answer()
            .unwrap()
            .expect("a network makes a socket");
        let to = ipv4((127, 0, 0, 1), port);
        let first = guest
            .tcp_tcp_socket_start_connect(socket, network, to.clone())
            .into_answer();
        let again = guest
            .tcp_tcp_socket_start_connect(socket, network, to)
            .into_answer();
        let bound_while_connecting = guest
            .tcp_tcp_socket_start_bind(socket, network, ipv4((0, 0, 0, 0), 0))
            .into_answer();
        let udp = guest
            .udp_create_socket_create_udp_socket(IpAddressFamily::Ipv4)
            .into_answer()
            .unwrap()
            .expect("a network makes a socket");
        let bound = guest
            .udp_udp_socket_start_bind(udp, network, ipv4((0, 0, 0, 0), 0))
            .into_answer();
        let rebound = guest
            .udp_udp_socket_start_bind(udp, network, ipv4((0, 0, 0, 0), 0))
            .into_answer();
        assert_eq!(
            (
                first.unwrap(),
                again.unwrap(),
                bound_while_connecting.unwrap(),
                bound.unwrap(),
                rebound.unwrap()
            ),
            (
                Ok(()),
                Err(ErrorCode::ConcurrencyConflict),
                Err(ErrorCode::ConcurrencyConflict),
                Ok(()),
                Err(ErrorCode::ConcurrencyConflict)
            ),
            "start-connect again, or a TCP start-bind, while a connect is under way, and \
             start-bind again before its finish, must answer concurrency-conflict"
        );
    }

    // @behavior NW-091
    #[test]
    fn a_tcp_sockets_pollable_waits_only_on_its_connect() {
        let (mut guest, listener) = guest_and_listener();
        let port = listener.local_addr().unwrap().port();
        let started = Instant::now();
        let answered = run_task(Some(started + Duration::from_secs(10)), || {
            let network = guest.instance_network_instance_network()?;
            let fresh = guest
                .tcp_create_socket_create_tcp_socket(IpAddressFamily::Ipv4)
                .into_answer()?
                .expect("a network makes a socket");
            let idle = guest.tcp_tcp_socket_subscribe(fresh)?;
            guest.poll_pollable_block(idle)?;
            let socket = guest
                .tcp_create_socket_create_tcp_socket(IpAddressFamily::Ipv4)
                .into_answer()?
                .expect("a network makes a socket");
            guest
                .tcp_tcp_socket_start_connect(socket, network, ipv4((127, 0, 0, 1), port))
                .into_answer()?
                .expect("the endpoint names the listener");
            let connecting = guest.tcp_tcp_socket_subscribe(socket)?;
            guest.poll_pollable_block(connecting)?;
            let (_, output) = guest
                .tcp_tcp_socket_finish_connect(socket)
                .into_answer()?
                .expect("the listener accepts");
            // The listener never reads, so once the host's buffers fill too the
            // send buffer stays full and a wait for room would hold; the
            // pollable answers anyway once connected.
            let mut full_for = 0;
            while full_for < 5 {
                match guest
                    .streams_output_stream_check_write(output)
                    .into_answer()?
                {
                    Ok(0) => {
                        full_for += 1;
                        std::thread::sleep(Duration::from_millis(50));
                    }
                    Ok(n) => {
                        full_for = 0;
                        guest
                            .streams_output_stream_write(output, vec![b'x'; n as usize])
                            .into_answer()?
                            .expect("the stream takes what it permitted");
                    }
                    Err(_) => return Err(wasmtime::Error::msg("the stream failed")),
                }
            }
            let connected = guest.tcp_tcp_socket_subscribe(socket)?;
            guest.poll_pollable_block(connected)?;
            guest.poll_pollable_block(connecting)?;
            Ok::<_, wasmtime::Error>(())
        });
        assert!(
            answered.is_ok() && started.elapsed() < Duration::from_secs(5),
            "pollable.block on a fresh socket's subscribe, on a connected socket's, and again on \
             the one taken while it connected, must answer at once, not {answered:?} after {:?}",
            started.elapsed()
        );
    }

    // @behavior NW-179
    #[test]
    fn a_keepalive_time_rounds_a_part_of_a_second_up() {
        let (mut guest, _listener) = guest_and_listener();
        let socket = guest
            .tcp_create_socket_create_tcp_socket(IpAddressFamily::Ipv4)
            .into_answer()
            .unwrap()
            .expect("a network makes a socket");
        let mut set_and_read = |idle: u64, interval: u64| {
            guest
                .tcp_tcp_socket_set_keep_alive_idle_time(socket, idle)
                .into_answer()
                .unwrap()
                .expect("a fresh socket takes the idle time");
            guest
                .tcp_tcp_socket_set_keep_alive_interval(socket, interval)
                .into_answer()
                .unwrap()
                .expect("a fresh socket takes the interval");
            (
                guest
                    .tcp_tcp_socket_keep_alive_idle_time(socket)
                    .into_answer()
                    .unwrap(),
                guest
                    .tcp_tcp_socket_keep_alive_interval(socket)
                    .into_answer()
                    .unwrap(),
            )
        };
        assert_eq!(
            [
                set_and_read(1_500_000_000, 500_000_000),
                set_and_read(2_000_000_000, 1)
            ],
            [
                (Ok(2_000_000_000), Ok(1_000_000_000)),
                (Ok(2_000_000_000), Ok(1_000_000_000))
            ],
            "tcp-socket keep-alive times of 1.5 s and 0.5 s must read back 2 s and 1 s, and whole \
             seconds unchanged, as wasmtime's pass through rustix, which rounds a part of a \
             second up"
        );
    }

    // @behavior NW-092
    #[test]
    fn a_write_past_the_last_check_write_permit_traps() {
        let (mut guest, listener) = guest_and_listener();
        let port = listener.local_addr().unwrap().port();
        let started = Instant::now();
        let (permitted, within, past) = run_task(Some(started + Duration::from_secs(10)), || {
            let network = guest.instance_network_instance_network()?;
            let socket = guest
                .tcp_create_socket_create_tcp_socket(IpAddressFamily::Ipv4)
                .into_answer()?
                .expect("a network makes a socket");
            guest
                .tcp_tcp_socket_start_connect(socket, network, ipv4((127, 0, 0, 1), port))
                .into_answer()?
                .expect("the endpoint names the listener");
            let connecting = guest.tcp_tcp_socket_subscribe(socket)?;
            guest.poll_pollable_block(connecting)?;
            let (_, output) = guest
                .tcp_tcp_socket_finish_connect(socket)
                .into_answer()?
                .expect("the listener accepts");
            let permitted = guest
                .streams_output_stream_check_write(output)
                .into_answer()?
                .expect("a connected stream permits");
            let within = guest
                .streams_output_stream_write(output, vec![b'x'])
                .into_answer();
            let past = guest
                .streams_output_stream_write(output, vec![b'x'])
                .into_answer();
            Ok::<_, wasmtime::Error>((permitted, within.is_ok(), past.is_err()))
        })
        .expect("the connect answers");
        assert!(
            permitted > 1 && within && past,
            "output-stream.write of one byte of what check-write permitted must succeed, and \
             another byte without another check-write must trap, as a permit is for the next \
             write only"
        );
    }

    // @behavior NW-159
    #[test]
    fn an_empty_write_leaves_the_check_write_permit() {
        let (mut guest, listener) = guest_and_listener();
        let port = listener.local_addr().unwrap().port();
        let started = Instant::now();
        let (empty, byte) = run_task(Some(started + Duration::from_secs(10)), || {
            let network = guest.instance_network_instance_network()?;
            let socket = guest
                .tcp_create_socket_create_tcp_socket(IpAddressFamily::Ipv4)
                .into_answer()?
                .expect("a network makes a socket");
            guest
                .tcp_tcp_socket_start_connect(socket, network, ipv4((127, 0, 0, 1), port))
                .into_answer()?
                .expect("the endpoint names the listener");
            let connecting = guest.tcp_tcp_socket_subscribe(socket)?;
            guest.poll_pollable_block(connecting)?;
            let (_, output) = guest
                .tcp_tcp_socket_finish_connect(socket)
                .into_answer()?
                .expect("the listener accepts");
            guest
                .streams_output_stream_check_write(output)
                .into_answer()?
                .expect("a connected stream permits");
            let empty = guest
                .streams_output_stream_write(output, Vec::new())
                .into_answer();
            let byte = guest
                .streams_output_stream_write(output, vec![b'x'])
                .into_answer();
            Ok::<_, wasmtime::Error>((empty.is_ok(), byte.is_ok()))
        })
        .expect("the connect answers");
        assert_eq!(
            (empty, byte),
            (true, true),
            "output-stream.write of no bytes must succeed and leave the check-write permit, so \
             one byte after it still writes, as wasmtime's write does"
        );
    }

    // @behavior NW-171
    #[test]
    fn a_write_the_socket_no_longer_takes_whole_fails() {
        let (mut guest, listener) = guest_and_listener();
        let port = listener.local_addr().unwrap().port();
        let started = Instant::now();
        let written = run_task(Some(started + Duration::from_secs(10)), || {
            let network = guest.instance_network_instance_network()?;
            let socket = guest
                .tcp_create_socket_create_tcp_socket(IpAddressFamily::Ipv4)
                .into_answer()?
                .expect("a network makes a socket");
            guest
                .tcp_tcp_socket_start_connect(socket, network, ipv4((127, 0, 0, 1), port))
                .into_answer()?
                .expect("the endpoint names the listener");
            let connecting = guest.tcp_tcp_socket_subscribe(socket)?;
            guest.poll_pollable_block(connecting)?;
            let (_, output) = guest
                .tcp_tcp_socket_finish_connect(socket)
                .into_answer()?
                .expect("the listener accepts");
            let permit = guest
                .streams_output_stream_check_write(output)
                .into_answer()?
                .expect("a connected stream permits");
            guest
                .tcp_tcp_socket_set_send_buffer_size(socket, 1)
                .into_answer()?
                .expect("a connected socket takes a send buffer size");
            let written = guest
                .streams_output_stream_write(output, vec![0; permit as usize])
                .into_answer()?;
            let after = guest
                .streams_output_stream_check_write(output)
                .into_answer()?;
            Ok::<_, wasmtime::Error>((written.is_ok(), matches!(after, Err(StreamError::Closed))))
        })
        .expect("the calls answer");
        let (mut peer, _) = listener.accept().expect("the listener accepts");
        peer.set_read_timeout(Some(Duration::from_millis(200)))
            .unwrap();
        let heard = std::io::Read::read(&mut peer, &mut [0; 16]).ok();
        assert_eq!(
            (written, heard),
            ((false, true), None),
            "output-stream.write of the whole check-write permit after the guest shrank the \
             socket's send buffer must fail before any byte leaves, rather than send the peer \
             part of it, and the next check-write must answer closed, as io.wit closes a stream \
             past last-operation-failed"
        );
    }

    // @behavior NW-182
    #[test]
    fn a_blocking_flush_after_the_send_side_shut_answers_closed() {
        let (mut guest, listener) = guest_and_listener();
        let port = listener.local_addr().unwrap().port();
        let started = Instant::now();
        let answers = run_task(Some(started + Duration::from_secs(10)), || {
            let network = guest.instance_network_instance_network()?;
            let socket = guest
                .tcp_create_socket_create_tcp_socket(IpAddressFamily::Ipv4)
                .into_answer()?
                .expect("a network makes a socket");
            guest
                .tcp_tcp_socket_start_connect(socket, network, ipv4((127, 0, 0, 1), port))
                .into_answer()?
                .expect("the endpoint names the listener");
            let connecting = guest.tcp_tcp_socket_subscribe(socket)?;
            guest.poll_pollable_block(connecting)?;
            let (_, output) = guest
                .tcp_tcp_socket_finish_connect(socket)
                .into_answer()?
                .expect("the listener accepts");
            guest
                .tcp_tcp_socket_shutdown(socket, ShutdownType::Send)
                .into_answer()?
                .expect("a connected socket shuts its send side");
            let nothing = guest
                .streams_output_stream_blocking_write_and_flush(output, Vec::new())
                .into_answer()?;
            Ok::<_, wasmtime::Error>(matches!(nothing, Err(StreamError::Closed)))
        });
        assert!(
            matches!(answers, Ok(true)),
            "output-stream.blocking-write-and-flush of no bytes after tcp-socket.shutdown of the \
             send side must answer closed, as wasmtime's first asks the stream for room, not \
             {answers:?}"
        );
    }

    // @behavior NW-186
    #[test]
    fn a_shutdown_of_the_receive_side_closes_the_input_stream() {
        let (mut guest, listener) = guest_and_listener();
        let port = listener.local_addr().unwrap().port();
        let started = Instant::now();
        let answers = run_task(Some(started + Duration::from_secs(10)), || {
            let network = guest.instance_network_instance_network()?;
            let socket = guest
                .tcp_create_socket_create_tcp_socket(IpAddressFamily::Ipv4)
                .into_answer()?
                .expect("a network makes a socket");
            guest
                .tcp_tcp_socket_start_connect(socket, network, ipv4((127, 0, 0, 1), port))
                .into_answer()?
                .expect("the endpoint names the listener");
            let connecting = guest.tcp_tcp_socket_subscribe(socket)?;
            guest.poll_pollable_block(connecting)?;
            let (input, _) = guest
                .tcp_tcp_socket_finish_connect(socket)
                .into_answer()?
                .expect("the listener accepts");
            let (mut peer, _) = listener.accept().expect("the listener accepts");
            std::io::Write::write_all(&mut peer, b"queued").expect("the peer sends");
            let readable = guest.streams_input_stream_subscribe(input)?;
            guest.poll_pollable_block(readable)?;
            guest
                .tcp_tcp_socket_shutdown(socket, ShutdownType::Receive)
                .into_answer()?
                .expect("a connected socket shuts its receive side");
            let read = guest.streams_input_stream_read(input, 16).into_answer()?;
            Ok::<_, wasmtime::Error>(matches!(read, Err(StreamError::Closed)))
        });
        assert!(
            matches!(answers, Ok(true)),
            "input-stream.read after tcp-socket.shutdown of the receive side must answer closed, \
             though bytes were queued, as tcp.wit closes the input stream and discards its queue, \
             not {answers:?}"
        );
    }

    // @behavior NW-171
    #[test]
    fn a_closed_output_streams_pollable_is_ready_at_once() {
        let mut guest = network_guest(network::Endpoint::new("127.0.0.1", 80).unwrap());
        // A refused lookup's file never polls writable, so only the stream's
        // own closed state can make its pollable ready.
        let lookup = guest
            .network()
            .resolve("unnamed.test")
            .expect("the lookup answers");
        let file = Arc::new(Mutex::new(lookup.alloc_file(guest.wasi().cred())));
        let output: OutputStream = guest
            .table_mut()
            .push(table::OutputStream::new(table::Stream::File(file)))
            .unwrap();
        let pollable = guest.streams_output_stream_subscribe(output).unwrap();
        let ready =
            |guest: &mut Invocation| guest.table_mut().get(pollable).unwrap().is_ready(None);
        let open = ready(&mut guest);
        guest
            .table_mut()
            .get(output)
            .unwrap()
            .closed
            .store(true, Ordering::Release);
        assert_eq!(
            (open, ready(&mut guest)),
            (false, true),
            "output-stream.subscribe of a stream that cannot take a write must not be ready, and \
             once the stream is closed it must be ready at once, as io.wit has a closed stream's \
             pollable"
        );
    }

    // @behavior NW-160
    #[test]
    fn a_connecting_socket_answers_its_state_before_a_zero_buffer_size() {
        let (mut guest, listener) = guest_and_listener();
        let port = listener.local_addr().unwrap().port();
        let answers = (|| {
            let network = guest.instance_network_instance_network()?;
            let socket = guest
                .tcp_create_socket_create_tcp_socket(IpAddressFamily::Ipv4)
                .into_answer()?
                .expect("a network makes a socket");
            guest
                .tcp_tcp_socket_start_connect(socket, network, ipv4((127, 0, 0, 1), port))
                .into_answer()?
                .expect("the endpoint names the listener");
            Ok::<_, wasmtime::Error>((
                guest
                    .tcp_tcp_socket_set_receive_buffer_size(socket, 0)
                    .into_answer()?,
                guest
                    .tcp_tcp_socket_set_send_buffer_size(socket, 0)
                    .into_answer()?,
            ))
        })()
        .expect("the connect starts");
        assert_eq!(
            answers,
            (Err(ErrorCode::InvalidState), Err(ErrorCode::InvalidState)),
            "tcp-socket set-receive-buffer-size and set-send-buffer-size of zero on a socket \
             still connecting must answer invalid-state, as wasmtime reads the socket before \
             the size"
        );
    }

    // @behavior NW-084
    #[test]
    fn a_datagram_past_what_udp_carries_answers_datagram_too_large() {
        let peer = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let port = peer.local_addr().unwrap().port();
        let mut guest = network_guest(
            network::Endpoint::new("127.0.0.1", port)
                .unwrap()
                .with_protocol(network::Protocol::Udp),
        );
        let started = Instant::now();
        let sent = run_task(Some(started + Duration::from_secs(10)), || {
            let network = guest.instance_network_instance_network()?;
            let socket = guest
                .udp_create_socket_create_udp_socket(IpAddressFamily::Ipv4)
                .into_answer()?
                .expect("a network makes a socket");
            let any = IpSocketAddress::Ipv4(Ipv4SocketAddress {
                port: 0,
                address: (0, 0, 0, 0),
            });
            guest
                .udp_udp_socket_start_bind(socket, network, any)
                .into_answer()?
                .expect("an ephemeral bind is the implicit one");
            guest
                .udp_udp_socket_finish_bind(socket)
                .into_answer()?
                .expect("the bind settles");
            let peer_address = IpSocketAddress::Ipv4(Ipv4SocketAddress {
                port,
                address: (127, 0, 0, 1),
            });
            let (_, outgoing) = guest
                .udp_udp_socket_stream(socket, Some(peer_address))
                .into_answer()?
                .expect("the udp endpoint names the peer");
            let room = guest.udp_outgoing_datagram_stream_subscribe(outgoing)?;
            guest.poll_pollable_block(room)?;
            guest
                .udp_outgoing_datagram_stream_check_send(outgoing)
                .into_answer()?
                .expect("the stream with room permits a datagram");
            let sent = guest
                .udp_outgoing_datagram_stream_send(
                    outgoing,
                    vec![OutgoingDatagram {
                        data: vec![0; MAX_DATAGRAM + 1],
                        remote_address: None,
                    }],
                )
                .into_answer()?;
            let elsewhere = IpSocketAddress::Ipv4(Ipv4SocketAddress {
                port,
                address: (127, 0, 0, 2),
            });
            guest
                .udp_outgoing_datagram_stream_check_send(outgoing)
                .into_answer()?
                .expect("the stream with room permits a datagram");
            let misaddressed = guest
                .udp_outgoing_datagram_stream_send(
                    outgoing,
                    vec![OutgoingDatagram {
                        data: vec![0; MAX_DATAGRAM + 1],
                        remote_address: Some(elsewhere),
                    }],
                )
                .into_answer()?;
            Ok::<_, wasmtime::Error>((sent, misaddressed))
        })
        .expect("the send answers");
        assert!(
            matches!(
                sent,
                (
                    Err(ErrorCode::DatagramTooLarge),
                    Err(ErrorCode::DatagramTooLarge)
                )
            ),
            "outgoing-datagram-stream.send of one byte past what IPv4 UDP carries must answer \
             datagram-too-large, as udp_sendmsg answers EMSGSIZE, even to an address other than \
             the peer, as wasmtime judges the size first, not {sent:?}"
        );
    }

    // @behavior NW-072
    #[test]
    fn a_datagram_send_past_its_permit_traps() {
        let peer = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let port = peer.local_addr().unwrap().port();
        let mut guest = network_guest(
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
                    .udp_create_socket_create_udp_socket(IpAddressFamily::Ipv4)
                    .into_answer()?
                    .expect("a network makes a socket");
                guest
                    .udp_udp_socket_start_bind(
                        socket,
                        network,
                        IpSocketAddress::Ipv4(Ipv4SocketAddress {
                            port: 0,
                            address: (0, 0, 0, 0),
                        }),
                    )
                    .into_answer()?
                    .expect("an ephemeral bind is the implicit one");
                guest
                    .udp_udp_socket_finish_bind(socket)
                    .into_answer()?
                    .expect("the bind settles");
                let (_incoming, outgoing) = guest
                    .udp_udp_socket_stream(
                        socket,
                        Some(IpSocketAddress::Ipv4(Ipv4SocketAddress {
                            port,
                            address: (127, 0, 0, 1),
                        })),
                    )
                    .into_answer()?
                    .expect("the udp endpoint names the peer");
                let room = guest.udp_outgoing_datagram_stream_subscribe(outgoing)?;
                guest.poll_pollable_block(room)?;
                let unpermitted = guest
                    .udp_outgoing_datagram_stream_send(outgoing, ping())
                    .into_answer()
                    .is_err();
                let permit = guest
                    .udp_outgoing_datagram_stream_check_send(outgoing)
                    .into_answer()?;
                let sent = guest
                    .udp_outgoing_datagram_stream_send(outgoing, ping())
                    .into_answer()?;
                let spent = guest
                    .udp_outgoing_datagram_stream_send(outgoing, ping())
                    .into_answer()
                    .is_err();
                wasmtime::Result::Ok((unpermitted, permit, sent, spent))
            },
        )
        .expect("the socket streams with its peer");
        assert_eq!(
            (unpermitted, permit, sent, spent),
            (true, Ok(16), Ok(1), true),
            "outgoing-datagram-stream.send must trap before any check-send and once its permit \
             is spent, and send what check-send permitted, sixteen while the queue has room"
        );
    }

    // @behavior NW-073
    #[test]
    fn a_blocking_write_past_4096_bytes_traps() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let mut guest = loopback_guest(listener.local_addr().unwrap().port());
        let answers = run_task(
            Some(Instant::now() + Duration::from_secs(10)),
            || -> wasmtime::Result<_> {
                let socket = guest
                    .tcp_create_socket_create_tcp_socket(IpAddressFamily::Ipv4)
                    .into_answer()?
                    .expect("a network makes a socket");
                let network = guest.instance_network_instance_network()?;
                let remote = IpSocketAddress::Ipv4(Ipv4SocketAddress {
                    port: listener.local_addr().unwrap().port(),
                    address: (127, 0, 0, 1),
                });
                guest
                    .tcp_tcp_socket_start_connect(socket, network, remote)
                    .into_answer()?
                    .expect("an endpoint names the address");
                let connecting = guest.tcp_tcp_socket_subscribe(socket)?;
                guest.poll_pollable_block(connecting)?;
                let (_input, output) = guest
                    .tcp_tcp_socket_finish_connect(socket)
                    .into_answer()?
                    .expect("the listener accepts");
                let past = guest
                    .streams_output_stream_blocking_write_and_flush(output, vec![0; 4097])
                    .into_answer()
                    .is_err();
                let within = guest
                    .streams_output_stream_blocking_write_and_flush(output, vec![0; 4096])
                    .into_answer()
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
        let mut guest = loopback_guest(listener.local_addr().unwrap().port());
        let started = Instant::now();
        let answer = run_task(Some(started + Duration::from_secs(10)), || {
            let socket = guest
                .tcp_create_socket_create_tcp_socket(IpAddressFamily::Ipv4)
                .into_answer()?
                .expect("a network makes a socket");
            let network = guest.instance_network_instance_network()?;
            let remote = IpSocketAddress::Ipv4(Ipv4SocketAddress {
                port: listener.local_addr().unwrap().port(),
                address: (127, 0, 0, 1),
            });
            guest
                .tcp_tcp_socket_start_connect(socket, network, remote)
                .into_answer()?
                .expect("an endpoint names the address");
            let connecting = guest.tcp_tcp_socket_subscribe(socket)?;
            guest.poll_pollable_block(connecting)?;
            let (input, _output) = guest
                .tcp_tcp_socket_finish_connect(socket)
                .into_answer()?
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
             hermetic profile must answer the clock once they pass, got {answer:?} after \
             {spent:?}"
        );
    }

    // @behavior RT-382
    #[test]
    fn a_clock_past_what_the_host_counts_waits_until_the_deadline() {
        let (mut store, _) = guest(Profile::Permissive, None);
        let guest = store.data_mut();
        let started = Instant::now();
        let waited = run_task(Some(started + Duration::from_millis(100)), || {
            let clock = guest.monotonic_clock_subscribe_duration(u64::MAX)?;
            guest.poll_pollable_block(clock)
        });
        assert!(
            waited.is_err() && started.elapsed() >= Duration::from_millis(100),
            "subscribe-duration of the largest duration through a permissive guest must hand out \
             a clock that only the deadline ends"
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
        let held = (1..crate::wasi::MAX_TABLE_ENTRIES)
            .map(|_| guest.monotonic_clock_subscribe_duration(0))
            .take_while(Result::is_ok)
            .count();
        let past = guest.monotonic_clock_subscribe_duration(0);
        assert_eq!(
            (first.is_ok(), held, past.is_err()),
            (true, crate::wasi::MAX_TABLE_ENTRIES - 1, true),
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
        let mut guest = limited_network_guest([endpoint], Some(3));
        assert_eq!(
            (
                guest
                    .tcp_create_socket_create_tcp_socket(IpAddressFamily::Ipv4)
                    .into_answer()
                    .ok()
                    .and_then(|answer| answer.err()),
                guest
                    .udp_create_socket_create_udp_socket(IpAddressFamily::Ipv4)
                    .into_answer()
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

    // @behavior NW-146
    #[test]
    fn a_lookup_or_a_request_past_the_open_file_limit_answers_an_error() {
        use super::super::bindings::wasi_http_types_0_2_12::{ErrorCode as HttpErrorCode, Scheme};
        use super::super::bindings::HttpHost;
        let endpoint = network::Endpoint::new("example.com", 80).unwrap();
        let mut guest = limited_network_guest([endpoint], Some(3));
        let network = guest.instance_network_instance_network().unwrap();
        let lookup = guest
            .ip_name_lookup_resolve_addresses(network, "example.com".to_string())
            .into_answer()
            .ok()
            .and_then(|answer| answer.err());
        let headers = guest.types_fields_new().unwrap();
        let headers = guest.take_handle(headers).unwrap();
        let request = guest.types_outgoing_request_new(headers).unwrap();
        guest
            .types_outgoing_request_set_scheme(request, Some(Scheme::HTTP))
            .unwrap()
            .unwrap();
        guest
            .types_outgoing_request_set_authority(request, Some("example.com".to_string()))
            .unwrap()
            .unwrap();
        let request = guest.take_handle(request).unwrap();
        let sent = guest
            .outgoing_handler_handle(request, None)
            .into_answer()
            .ok()
            .and_then(|answer| answer.err());
        assert_eq!(
            (lookup, sent),
            (
                Some(ErrorCode::NewSocketLimit),
                Some(HttpErrorCode::ConnectionLimitReached)
            ),
            "resolve-addresses and outgoing-handler.handle with the open file limit filled by \
             the standard streams must answer new-socket-limit and connection-limit-reached, \
             not trap"
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
        let mut guest = limited_network_guest([endpoint], Some(4));
        let started = Instant::now();
        let connected = run_task(Some(started + Duration::from_secs(10)), || {
            let network = guest.instance_network_instance_network()?;
            let socket = guest
                .tcp_create_socket_create_tcp_socket(IpAddressFamily::Ipv4)
                .into_answer()?
                .expect("one file is left for the socket");
            let remote = IpSocketAddress::Ipv4(Ipv4SocketAddress {
                port,
                address: (127, 0, 0, 1),
            });
            guest
                .tcp_tcp_socket_start_connect(socket, network, remote)
                .into_answer()?
                .expect("the endpoint names the listener");
            let connecting = guest.tcp_tcp_socket_subscribe(socket)?;
            guest.poll_pollable_block(connecting)?;
            Ok::<_, wasmtime::Error>(
                guest
                    .tcp_tcp_socket_finish_connect(socket)
                    .into_answer()?
                    .is_ok(),
            )
        });
        assert!(
            matches!(connected, Ok(true)),
            "a TCP socket made with one file left under the open file limit must connect and \
             hand out its streams, as Linux's connect takes no fd, not {connected:?}"
        );
    }

    /// The preview2 functions a test answers before a Guest Binary imports
    /// them, each beside the test that holds the answer.
    const UNIMPORTED_ANSWERS: [(&str, &str, &str); 1] = [(
        "wasi:sockets/tcp@0.2.12",
        "[method]tcp-socket.remote-address",
        "a_looked_up_name_connects_and_reads_back_its_virtual_address",
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
        let unimported = UNIMPORTED_ANSWERS
            .iter()
            .map(|(module, name, _)| (module.to_string(), name.to_string()))
            .collect::<std::collections::BTreeSet<_>>();
        let expected = imported
            .union(&unimported)
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
    fn a_handle_numbers_apart_from_the_descriptors() {
        let (mut store, _) = guest(Profile::Hermetic, None);
        let guest = store.data_mut();
        let first = guest.instance_network_instance_network().unwrap();
        let next_descriptor = guest.wasi().descriptors().lowest_unused();
        guest.network_network_drop(first).unwrap();
        let again = guest.instance_network_instance_network().unwrap();
        assert_eq!(
            (first.0, next_descriptor, again.0),
            (1, Some(3), 1),
            "a guest holding only stdin, stdout, and stderr must take 1 for its first preview2 \
             handle, still find 3 as preview1's next descriptor, and take 1 again once it is freed"
        );
    }

    // @behavior RT-383
    #[test]
    fn preview1_reaches_no_socket_and_no_preview2_handle() {
        let (mut store, instance) = guest(Profile::Hermetic, None);
        let handles: Vec<i32> = (0..3)
            .map(|_| call::<(), i32>(&mut store, &instance, "network", ()).expect("a network"))
            .collect();
        let mut on = |name: &str, fd: i32| {
            call::<i32, i32>(&mut store, &instance, name, fd).expect("the call answers")
        };
        let calls = ["sock_accept", "sock_recv", "sock_send", "sock_shutdown"];
        let on_stdout = calls.map(|name| on(name, 1));
        let on_handle = (on("fdstat", 3), on("sock_shutdown", 3));
        assert_eq!(
            (handles, on_stdout, on_handle),
            (vec![1, 2, 3], [57; 4], (8, 8)),
            "sock_accept, sock_recv, sock_send, and sock_shutdown on stdout must answer \
             ENOTSOCK, as a preview1 descriptor is always a file, and preview1 calls on 3, a \
             number only a preview2 handle holds, must answer EBADF"
        );
    }

    // @behavior RT-348
    #[test]
    fn a_drop_of_a_handle_as_another_kind_traps() {
        let (mut store, _) = guest(Profile::Hermetic, None);
        let guest = store.data_mut();
        let network = guest.instance_network_instance_network().unwrap();
        let wrong = guest.tcp_tcp_socket_drop(TcpSocket(network.0)).is_err();
        let kept = guest.table_mut().get(network).is_ok();
        assert!(
            wrong && kept,
            "tcp-socket's drop of a network handle must trap and leave the network held"
        );
    }

    // @behavior RT-348
    #[test]
    fn a_handle_of_another_kind_where_a_network_is_taken_traps() {
        let (mut store, instance) = guest(Profile::Hermetic, None);
        let pollable = call::<i64, i32>(&mut store, &instance, "subscribe", 0).expect("a pollable");
        let answer = call::<(i32, i32), ()>(&mut store, &instance, "resolve", (pollable, 0));
        assert!(
            answer.is_err(),
            "resolve-addresses handed a pollable where its network is taken must trap, not \
             answer an error-code"
        );
    }
}
