//! What the Wasm Driver answers each preview2 import. Without a network Grant
//! the guest makes no socket, as WASI says a host without a network answers;
//! with one, each connect passes the Invocation's ruleset and dials through
//! its Protocol Family. The clock subscriptions, the streams, and `poll` wait
//! on the runtime's wait primitive.

use std::net::Shutdown;
use std::time::{Duration, Instant};

use kobako_runtime::net::{sock_alloc_file, SOCKET_BUFFER};
use kobako_runtime::vfs::{FilesystemError, PollMask};
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
use super::bindings::{ClocksHost, IoHost, SocketsHost};
use super::sockets::{
    from_socket_addr, hide_host_address, lookup_error_code, to_error_code, to_socket_addr,
};
use super::table::{self, Resource, SocketFile, Stream};
use crate::invocation::Invocation;
use crate::trap::TimeoutTrap;

impl Invocation {
    /// A pollable on the guest's own clocks: ready at once on frozen ones,
    /// and `nanos` of host time from now on live ones.
    fn subscribe_clock(&mut self, nanos: u64) -> wasmtime::Result<Pollable> {
        let pollable = table::Pollable::Clock {
            at: Instant::now() + Duration::from_nanos(nanos),
            frozen: !self.wasi().clocks().advances(),
        };
        Ok(Pollable(
            self.wasi_mut()
                .table_mut()
                .push(Resource::Pollable(pollable))?,
        ))
    }

    /// Wait until `ready` answers, or trap as the deadline or an interrupt
    /// ends the run; the wait sleeps until the soonest pollable is due, or a
    /// queue `ready` registered on wakes.
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
                Err(_) => return Err(wasmtime::Error::msg("the guest's wait was interrupted")),
            }
        }
    }

    /// The handle is the capability to connect or look up, so it must name a
    /// network, as WASI's host checks it.
    fn network_held(&self, network: Network) -> wasmtime::Result<()> {
        match self.wasi().table().get(network.0)? {
            Resource::Network => Ok(()),
            _ => Err(wasmtime::Error::msg(
                "the guest named a handle that is not a network",
            )),
        }
    }

    fn push(&mut self, resource: Resource) -> wasmtime::Result<u32> {
        self.wasi_mut().table_mut().push(resource)
    }

    pub(super) fn pollable_on(
        &mut self,
        stream: Stream,
        mask: PollMask,
    ) -> wasmtime::Result<Pollable> {
        Ok(Pollable(self.push(Resource::Pollable(
            table::Pollable::Stream(stream, mask),
        ))?))
    }

    fn tcp(&mut self, handle: TcpSocket) -> wasmtime::Result<&mut table::TcpSocket> {
        match self.wasi_mut().table_mut().get_mut(handle.0)? {
            Resource::TcpSocket(socket) => Ok(socket),
            _ => Err(wasmtime::Error::msg(
                "the guest named a handle that is not a tcp-socket",
            )),
        }
    }

    fn stream(&self, handle: u32) -> wasmtime::Result<Stream> {
        match self.wasi().table().get(handle)? {
            Resource::InputStream(stream) | Resource::OutputStream(stream) => Ok(stream.clone()),
            _ => Err(wasmtime::Error::msg(
                "the guest named a handle that is not a stream",
            )),
        }
    }

    /// A stream's failure as WASI hands it out: the end as `closed`, anything
    /// else as an error the guest may read.
    fn stream_error(&mut self, err: FilesystemError) -> wasmtime::Result<StreamError> {
        match err {
            FilesystemError::BrokenPipe => Ok(StreamError::Closed),
            err => Ok(StreamError::LastOperationFailed(Error(
                self.push(Resource::Error(err))?,
            ))),
        }
    }

    /// Read what the stream holds, up to `len`, without waiting: nothing yet
    /// reads as an empty list, and the end as `closed`.
    fn read_now(
        &mut self,
        stream: &Stream,
        len: u64,
    ) -> wasmtime::Result<Result<Vec<u8>, StreamError>> {
        let mut buf = vec![0; len.min(SOCKET_BUFFER as u64) as usize];
        let read = stream.read(&mut buf);
        match read {
            Ok(0) if !buf.is_empty() => Ok(Err(StreamError::Closed)),
            Ok(n) => {
                buf.truncate(n);
                Ok(Ok(buf))
            }
            Err(FilesystemError::WouldBlock) => Ok(Ok(Vec::new())),
            Err(err) => self.stream_error(err).map(Err),
        }
    }

    /// Wait until `stream`'s poll finds `mask`, or an error or a hang-up.
    fn wait_on(&self, stream: &Stream, mask: PollMask) -> wasmtime::Result<()> {
        let pollable = table::Pollable::Stream(stream.clone(), mask);
        self.wait_for(None, |table| pollable.ready(table, true).then_some(()))
    }
}

impl SocketsHost for Invocation {
    fn instance_network_instance_network(&mut self) -> wasmtime::Result<Network> {
        Ok(Network(
            self.wasi_mut().table_mut().push(Resource::Network)?,
        ))
    }

    fn network_network_drop(&mut self, handle: Network) -> wasmtime::Result<()> {
        self.wasi_mut().table_mut().delete(handle.0).map(drop)
    }

    /// A lookup of `name`, answered through the stream it hands out. Without a
    /// network Grant no resolver answers, as WASI's `permanent-resolver-failure`
    /// says of a host without one.
    fn ip_name_lookup_resolve_addresses(
        &mut self,
        network: Network,
        name: String,
    ) -> wasmtime::Result<Result<ResolveAddressStream, ErrorCode>> {
        self.network_held(network)?;
        let Some(namespace) = self.wasi().table().network() else {
            return Ok(Err(ErrorCode::PermanentResolverFailure));
        };
        match namespace.resolve(&name) {
            Ok(lookup) => Ok(Ok(ResolveAddressStream(
                self.push(Resource::ResolveStream(lookup, false))?,
            ))),
            Err(err) => Ok(Err(lookup_error_code(err, true))),
        }
    }

    /// The name's one virtual address, then the end; a lookup still under way
    /// answers `would-block`.
    fn ip_name_lookup_resolve_address_stream_resolve_next_address(
        &mut self,
        this: ResolveAddressStream,
    ) -> wasmtime::Result<Result<Option<IpAddress>, ErrorCode>> {
        let Resource::ResolveStream(lookup, handed) =
            self.wasi_mut().table_mut().get_mut(this.0)?
        else {
            return Err(wasmtime::Error::msg(
                "the guest named a handle that is not a resolve-address-stream",
            ));
        };
        if *handed {
            return Ok(Ok(None));
        }
        let lookup = lookup.clone();
        let real = match lookup.answer() {
            None => return Ok(Err(ErrorCode::WouldBlock)),
            Some(Err(err)) => return Ok(Err(lookup_error_code(err, false))),
            Some(Ok(real)) => real,
        };
        let namespace = self
            .wasi()
            .table()
            .network()
            .expect("a lookup is only made with a network");
        let Some(at) = namespace.virtual_address(lookup.name(), real) else {
            return Ok(Err(ErrorCode::TemporaryResolverFailure));
        };
        if let Resource::ResolveStream(_, handed) = self.wasi_mut().table_mut().get_mut(this.0)? {
            *handed = true;
        }
        let [a, b, c, d] = at.octets();
        Ok(Ok(Some(IpAddress::Ipv4((a, b, c, d)))))
    }

    fn ip_name_lookup_resolve_address_stream_subscribe(
        &mut self,
        this: ResolveAddressStream,
    ) -> wasmtime::Result<Pollable> {
        let lookup = match self.wasi().table().get(this.0)? {
            Resource::ResolveStream(lookup, _) => lookup.clone(),
            _ => {
                return Err(wasmtime::Error::msg(
                    "the guest named a handle that is not a resolve-address-stream",
                ))
            }
        };
        Ok(Pollable(self.push(Resource::Pollable(
            table::Pollable::Lookup(lookup),
        ))?))
    }

    fn ip_name_lookup_resolve_address_stream_drop(
        &mut self,
        handle: ResolveAddressStream,
    ) -> wasmtime::Result<()> {
        self.wasi_mut().table_mut().delete(handle.0).map(drop)
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
        Ok(Ok(TcpSocket(
            self.push(Resource::TcpSocket(table::TcpSocket::Unbound))?,
        )))
    }

    /// A connect passes the Invocation's namespace on what the guest named,
    /// which dials that exact address on a worker, so this answers at once.
    fn tcp_tcp_socket_start_connect(
        &mut self,
        this: TcpSocket,
        network: Network,
        remote_address: IpSocketAddress,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        self.network_held(network)?;
        if !matches!(self.tcp(this)?, table::TcpSocket::Unbound) {
            return Ok(Err(ErrorCode::InvalidState));
        }
        let Some(addr) = to_socket_addr(&remote_address) else {
            return Ok(Err(ErrorCode::InvalidArgument));
        };
        let cred = self.wasi().descriptors().cred();
        let connected = self
            .wasi()
            .table()
            .network()
            .expect("a socket is only made with a network")
            .connect(cred, addr, self.deadline());
        let sock = match connected {
            Ok(sock) => sock,
            Err(err) => return Ok(Err(to_error_code(&err))),
        };
        let file = sock_alloc_file(&sock, cred);
        *self.tcp(this)? = table::TcpSocket::Connecting(SocketFile::new(sock, file, addr));
        Ok(Ok(()))
    }

    /// The streams of a connect that landed, as WASI hands them out once; one
    /// still under way answers `would-block`, and a failed one closes the
    /// socket.
    fn tcp_tcp_socket_finish_connect(
        &mut self,
        this: TcpSocket,
    ) -> wasmtime::Result<Result<(InputStream, OutputStream), ErrorCode>> {
        let socket = match self.tcp(this)? {
            table::TcpSocket::Connecting(socket) => socket.clone(),
            _ => return Ok(Err(ErrorCode::NotInProgress)),
        };
        match socket.sock.finish_connect() {
            Ok(()) => {
                *self.tcp(this)? = table::TcpSocket::Connected(socket.clone());
                let stream = Stream::Socket(socket);
                let input = InputStream(self.push(Resource::InputStream(stream.clone()))?);
                let output = OutputStream(self.push(Resource::OutputStream(stream))?);
                Ok(Ok((input, output)))
            }
            Err(FilesystemError::WouldBlock) => Ok(Err(ErrorCode::WouldBlock)),
            Err(err) => {
                *self.tcp(this)? = table::TcpSocket::Closed;
                Ok(Err(to_error_code(&err)))
            }
        }
    }

    /// Ready once a connect settles; a socket not connecting is ready at once.
    fn tcp_tcp_socket_subscribe(&mut self, this: TcpSocket) -> wasmtime::Result<Pollable> {
        let socket = match self.tcp(this)? {
            table::TcpSocket::Connecting(socket) | table::TcpSocket::Connected(socket) => {
                Some(socket.clone())
            }
            _ => None,
        };
        match socket {
            Some(socket) => self.pollable_on(Stream::Socket(socket), PollMask::OUT),
            None => Ok(Pollable(
                self.push(Resource::Pollable(table::Pollable::Ready))?,
            )),
        }
    }

    fn tcp_tcp_socket_shutdown(
        &mut self,
        this: TcpSocket,
        shutdown_type: ShutdownType,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        let table::TcpSocket::Connected(socket) = self.tcp(this)? else {
            return Ok(Err(ErrorCode::InvalidState));
        };
        socket.sock.shutdown(match shutdown_type {
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
        match self.tcp(this)? {
            table::TcpSocket::Connected(socket) => Ok(Ok(from_socket_addr(socket.peer))),
            _ => Ok(Err(ErrorCode::InvalidState)),
        }
    }

    /// The guest's own end, which shows no host address.
    fn tcp_tcp_socket_local_address(
        &mut self,
        this: TcpSocket,
    ) -> wasmtime::Result<Result<IpSocketAddress, ErrorCode>> {
        match self.tcp(this)? {
            table::TcpSocket::Connected(socket) => Ok(socket
                .sock
                .local_addr()
                .map(hide_host_address)
                .ok_or(ErrorCode::InvalidState)),
            _ => Ok(Err(ErrorCode::InvalidState)),
        }
    }

    fn tcp_tcp_socket_address_family(
        &mut self,
        this: TcpSocket,
    ) -> wasmtime::Result<IpAddressFamily> {
        self.tcp(this)?;
        Ok(IpAddressFamily::Ipv4)
    }

    fn tcp_tcp_socket_is_listening(&mut self, this: TcpSocket) -> wasmtime::Result<bool> {
        self.tcp(this)?;
        Ok(false)
    }

    /// The guest only connects out, so a bind or a listen is refused, as
    /// Landlock's `BIND_*` rights refuse one.
    fn tcp_tcp_socket_start_bind(
        &mut self,
        this: TcpSocket,
        _network: Network,
        _local_address: IpSocketAddress,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        self.tcp(this)?;
        Ok(Err(ErrorCode::AccessDenied))
    }

    fn tcp_tcp_socket_start_listen(
        &mut self,
        this: TcpSocket,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        self.tcp(this)?;
        Ok(Err(ErrorCode::AccessDenied))
    }

    fn tcp_tcp_socket_drop(&mut self, handle: TcpSocket) -> wasmtime::Result<()> {
        self.wasi_mut().table_mut().delete(handle.0).map(drop)
    }
}

impl IoHost for Invocation {
    fn streams_input_stream_read(
        &mut self,
        this: InputStream,
        len: u64,
    ) -> wasmtime::Result<Result<Vec<u8>, StreamError>> {
        let stream = self.stream(this.0)?;
        self.read_now(&stream, len)
    }

    fn streams_input_stream_blocking_read(
        &mut self,
        this: InputStream,
        len: u64,
    ) -> wasmtime::Result<Result<Vec<u8>, StreamError>> {
        let stream = self.stream(this.0)?;
        self.wait_on(&stream, PollMask::IN)?;
        self.read_now(&stream, len)
    }

    fn streams_input_stream_subscribe(&mut self, this: InputStream) -> wasmtime::Result<Pollable> {
        let stream = self.stream(this.0)?;
        self.pollable_on(stream, PollMask::IN)
    }

    fn streams_input_stream_drop(&mut self, handle: InputStream) -> wasmtime::Result<()> {
        self.wasi_mut().table_mut().delete(handle.0).map(drop)
    }

    /// How many bytes a write takes now; a failed or shut-down stream answers
    /// its failure.
    fn streams_output_stream_check_write(
        &mut self,
        this: OutputStream,
    ) -> wasmtime::Result<Result<u64, StreamError>> {
        let stream = self.stream(this.0)?;
        let written = stream.write(&[]);
        match written {
            Ok(_) => Ok(Ok(stream.room() as u64)),
            Err(err) => self.stream_error(err).map(Err),
        }
    }

    fn streams_output_stream_write(
        &mut self,
        this: OutputStream,
        contents: Vec<u8>,
    ) -> wasmtime::Result<Result<(), StreamError>> {
        let written = self.stream(this.0)?.write(&contents);
        match written {
            Ok(n) if n == contents.len() => Ok(Ok(())),
            Ok(_) | Err(FilesystemError::WouldBlock) => Err(wasmtime::Error::msg(
                "output-stream.write was handed more than check-write allowed",
            )),
            Err(err) => self.stream_error(err).map(Err),
        }
    }

    /// Write every byte, waiting for room as it goes, then for the send
    /// buffer to drain.
    fn streams_output_stream_blocking_write_and_flush(
        &mut self,
        this: OutputStream,
        contents: Vec<u8>,
    ) -> wasmtime::Result<Result<(), StreamError>> {
        let stream = self.stream(this.0)?;
        let mut rest = &contents[..];
        while !rest.is_empty() {
            self.wait_on(&stream, PollMask::OUT)?;
            let written = stream.write(rest);
            match written {
                Ok(n) => rest = &rest[n..],
                Err(FilesystemError::WouldBlock) => {}
                Err(err) => return self.stream_error(err).map(Err),
            }
        }
        self.streams_output_stream_blocking_flush(this)
    }

    /// The writer sends as the buffer fills, so a flush starts nothing.
    fn streams_output_stream_flush(
        &mut self,
        this: OutputStream,
    ) -> wasmtime::Result<Result<(), StreamError>> {
        self.stream(this.0)?;
        Ok(Ok(()))
    }

    /// Wait until what was written has left for the host.
    fn streams_output_stream_blocking_flush(
        &mut self,
        this: OutputStream,
    ) -> wasmtime::Result<Result<(), StreamError>> {
        let stream = self.stream(this.0)?;
        // Registering through the stream's poll puts the wait on its queue,
        // which a socket's writer wakes after each chunk it sends.
        self.wait_for(None, |table| {
            let failed = stream.poll(table).contains(PollMask::ERR);
            (failed || stream.drained()).then_some(())
        })?;
        let written = stream.write(&[]);
        match written {
            Ok(_) => Ok(Ok(())),
            Err(err) => self.stream_error(err).map(Err),
        }
    }

    fn streams_output_stream_subscribe(
        &mut self,
        this: OutputStream,
    ) -> wasmtime::Result<Pollable> {
        let stream = self.stream(this.0)?;
        self.pollable_on(stream, PollMask::OUT)
    }

    fn streams_output_stream_drop(&mut self, handle: OutputStream) -> wasmtime::Result<()> {
        self.wasi_mut().table_mut().delete(handle.0).map(drop)
    }

    /// The failure's POSIX name, so no host text reaches the guest.
    fn error_error_to_debug_string(&mut self, this: Error) -> wasmtime::Result<String> {
        match self.wasi().table().get(this.0)? {
            Resource::Error(err) => Ok(err.posix_name().unwrap_or("EIO").to_string()),
            _ => Err(wasmtime::Error::msg(
                "the guest named a handle that is not an error",
            )),
        }
    }

    fn error_error_drop(&mut self, handle: Error) -> wasmtime::Result<()> {
        self.wasi_mut().table_mut().delete(handle.0).map(drop)
    }

    fn poll_pollable_ready(&mut self, this: Pollable) -> wasmtime::Result<bool> {
        let pollable = self.wasi().table().pollable(this.0)?;
        Ok(pollable.ready(None, pollable.is_producer()))
    }

    fn poll_pollable_block(&mut self, this: Pollable) -> wasmtime::Result<()> {
        let pollable = self.wasi().table().pollable(this.0)?;
        let producer = pollable.is_producer();
        self.wait_for(pollable.due(), |table| {
            pollable.ready(table, producer).then_some(())
        })
    }

    /// The indices of the pollables ready, once one is, as WASI's `poll`
    /// answers; an empty list traps, as WASI says it does.
    fn poll_poll(&mut self, r#in: Vec<Pollable>) -> wasmtime::Result<Vec<u32>> {
        if r#in.is_empty() {
            return Err(wasmtime::Error::msg("poll was handed no pollable"));
        }
        let pollables = r#in
            .iter()
            .map(|handle| self.wasi().table().pollable(handle.0))
            .collect::<wasmtime::Result<Vec<_>>>()?;
        // A wait beside a host-side producer takes host time, so a frozen clock
        // in it elapses as a live one does.
        let producer = pollables.iter().any(table::Pollable::is_producer);
        let due = pollables.iter().filter_map(table::Pollable::due).min();
        self.wait_for(due, |mut table| {
            let ready = (0..pollables.len() as u32)
                .filter(|i| pollables[*i as usize].ready(table.as_deref_mut(), producer))
                .collect::<Vec<_>>();
            (!ready.is_empty()).then_some(ready)
        })
    }

    fn poll_pollable_drop(&mut self, handle: Pollable) -> wasmtime::Result<()> {
        self.wasi_mut().table_mut().delete(handle.0).map(drop)
    }
}

impl ClocksHost for Invocation {
    fn monotonic_clock_subscribe_duration(
        &mut self,
        when: monotonic_clock::Duration,
    ) -> wasmtime::Result<Pollable> {
        self.subscribe_clock(when)
    }

    /// An instant counts from preview1's `clock_time_get` origin, so both
    /// encodings name one clock.
    fn monotonic_clock_subscribe_instant(
        &mut self,
        when: monotonic_clock::Instant,
    ) -> wasmtime::Result<Pollable> {
        let now = self.wasi().clocks().monotonic_now();
        self.subscribe_clock(when.saturating_sub(now))
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
    use crate::wasi::{sandbox_workers, Wasi};
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
            (call $poll (i32.const 64) (local.get 1) (i32.const 32))))
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
            profile,
            security: Vec::new(),
        };
        let wasi = Wasi::for_invocation(&config, Vec::new(), &[], &sandbox_workers())
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
        let config = Config {
            timeout: None,
            setup_timeout: None,
            memory_limit: None,
            stdout_limit: None,
            stderr_limit: None,
            open_file_limit: None,
            profile: Profile::Hermetic,
            security: Vec::new(),
        };
        let mut entry = network::Builder::default();
        entry.endpoint(endpoint);
        let grant = Grant::Network(network::Network::new("local", Arc::new(entry)).unwrap());
        let wasi = Wasi::for_invocation(&config, Vec::new(), &[grant], &sandbox_workers())
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
    fn a_resource_counts_against_the_open_file_limit() {
        let (mut store, instance) = guest(Profile::Hermetic, Some(3));
        assert!(
            call::<i64, i32>(&mut store, &instance, "subscribe", 0).is_err(),
            "subscribe-duration with the open file limit filled by the standard streams must trap"
        );
    }
}
