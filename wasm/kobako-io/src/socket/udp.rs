//! `UDPSocket` over `wasi:sockets/udp`. The guest only sends to a peer it
//! names: a socket binds to any address on an ephemeral port, then streams
//! with the one peer a `connect` names, as Linux's connected UDP socket does.

use std::cell::RefCell;
use std::ffi::CStr;
use std::net::{Ipv4Addr, Shutdown};
use std::task::Poll;

use beni::prelude::*;
use beni::{Error, IntoValue, Mrb, RClass, TryConvert, TypedData, Value};

use super::address::{local_of, no_limit, peer_of, to_ip_socket_address};
use super::addrinfo::{IPPROTO_UDP, SOCK_DGRAM};
use super::basic::{
    direct_args, ensure_flags_carried, exception_option, open_socket_of, ready_of, IoState,
    Readiness, SocketIo, Wait,
};
use super::ip::{local_end_of, to_addr_array};
use super::{bind, family_of, to_error, Family, AF_INET};
use crate::errno;
use crate::io::{would_wait, Direction};
use crate::wasi::wait_within;
use crate::wasi::wasi::sockets::network::{
    ErrorCode, IpAddressFamily, IpSocketAddress, Ipv4SocketAddress,
};
use crate::wasi::wasi::sockets::udp::{
    IncomingDatagramStream, OutgoingDatagram, OutgoingDatagramStream, UdpSocket,
};
use crate::wasi::wasi::sockets::udp_create_socket::create_udp_socket;

const UDP: &str = "UDPSocket";

/// A datagram received, and the end that sent it.
type Datagram = (Vec<u8>, (Ipv4Addr, u16));

/// The streams with one peer, dropped before their socket, as WASI asks of a
/// child resource.
struct Streams {
    incoming: IncomingDatagramStream,
    outgoing: OutgoingDatagramStream,
    peer: (Ipv4Addr, u16),
}

struct OpenSocket {
    streams: Option<Streams>,
    socket: UdpSocket,
    /// `shutdown` shut the sending side, so a send answers `EPIPE`. WASI 0.2
    /// names no shutdown for UDP, so the guest keeps it itself.
    send_shut: bool,
}

impl OpenSocket {
    /// The streams a send reaches its peer through, or the `Errno` name that
    /// refuses it: an address other than the peer first, as the socket hooks
    /// judge before the protocol; then no peer, then a shut sending side, in
    /// `udp_sendmsg`'s order. `names_peer` says whether the address a send
    /// names is the peer, `None` when it names none.
    fn streams_for_send(&self, names_peer: Option<bool>) -> Result<&Streams, &'static CStr> {
        match (&self.streams, names_peer) {
            (_, Some(false)) => Err(c"EACCES"),
            (None, _) => Err(c"EDESTADDRREQ"),
            _ if self.send_shut => Err(c"EPIPE"),
            (Some(streams), _) => Ok(streams),
        }
    }
}

/// What a `UDPSocket` stands for, kept in its payload so a script's instance
/// variables cannot rewrite it; `None` once closed.
#[beni::wrap(class = "UDPSocket", name = "Kobako::UDPSocket")]
struct Datagrams {
    open: RefCell<Option<OpenSocket>>,
    io: IoState,
}

impl SocketIo for Datagrams {
    fn io(&self) -> &IoState {
        &self.io
    }

    /// The next datagram, cut to `max`, as a datagram socket's read fills
    /// CRuby's buffer with one; a datagram socket has no end.
    fn receive(&self, mrb: &Mrb, max: usize, wait: Wait) -> Result<Poll<Option<Vec<u8>>>, Error> {
        let received = self.with(mrb, |open| receive_from(mrb, open, max, "read(2)", wait))?;
        Ok(received.map(|(data, _)| Some(data)))
    }

    /// `bytes` as one datagram to the peer, as `write(2)` on a connected
    /// datagram socket sends; with no peer it raises `Errno::EDESTADDRREQ`,
    /// carrying no call, as CRuby's `IO#write` does.
    fn send(&self, mrb: &Mrb, bytes: &[u8], wait: Wait) -> Result<Poll<usize>, Error> {
        let sent = self.with(mrb, |open| match open.streams_for_send(None) {
            Ok(streams) => send_datagram(mrb, streams, bytes.to_vec(), "write(2)", wait),
            Err(name) => Err(errno::error_alone(mrb, name)),
        })?;
        Ok(sent.map(|()| bytes.len()))
    }

    /// Mark the sending side shut too, as Linux's `inet_shutdown` does, and
    /// then raise `Errno::ENOTCONN` for one with no peer, which keeps the
    /// marks. Shutting the sending side refuses later sends; what arrives
    /// still reads.
    fn protocol_shutdown(&self, mrb: &Mrb, how: Shutdown) -> Result<(), Error> {
        self.with(mrb, |open| {
            open.send_shut |= how != Shutdown::Read;
            match open.streams {
                Some(_) => Ok(()),
                None => Err(errno::error_by_name(mrb, c"ENOTCONN", "shutdown(2)")),
            }
        })
    }

    /// The socket's own end; one not bound yet answers the unspecified
    /// address.
    fn local_end(&self, mrb: &Mrb) -> Result<(Ipv4Addr, u16), Error> {
        let local = self.with(mrb, |open| Ok(open.socket.local_address()))?;
        local_end_of(mrb, local, UDP)
    }

    fn peer_end(&self, mrb: &Mrb) -> Result<(Ipv4Addr, u16), Error> {
        self.with(mrb, |open| match &open.streams {
            Some(streams) => Ok(streams.peer),
            None => Err(errno::error_by_name(mrb, c"ENOTCONN", "getpeername(2)")),
        })
    }

    fn kind(&self) -> (i64, i64) {
        (SOCK_DGRAM, IPPROTO_UDP)
    }

    fn handle(&self, mrb: &Mrb) -> Result<u32, Error> {
        self.with(mrb, |open| Ok(open.socket.handle()))
    }

    /// A socket with no peer never reads and answers a write at once, as a
    /// write raises `Errno::EDESTADDRREQ` without waiting.
    fn protocol_readiness(&self, mrb: &Mrb, direction: Direction) -> Result<Readiness, Error> {
        self.with(mrb, |open| {
            Ok(match (direction, &open.streams) {
                (Direction::Read, Some(streams)) => {
                    Readiness::Pending(streams.incoming.subscribe())
                }
                (Direction::Read, None) => Readiness::Inert,
                (Direction::Write, Some(streams)) if !open.send_shut => {
                    Readiness::Pending(streams.outgoing.subscribe())
                }
                (Direction::Write, _) => Readiness::Ready,
            })
        })
    }

    fn close(&self) {
        self.open.borrow_mut().take();
    }

    fn is_closed(&self) -> bool {
        self.open.borrow().is_none()
    }
}

/// What `call` answers for the UDP socket `value` stands for, `None` when it
/// stands for no `UDPSocket`; a closed one raises `IOError`.
pub(super) fn with_udp_socket<T>(
    mrb: &Mrb,
    value: Value,
    call: impl FnOnce(&UdpSocket) -> Result<T, Error>,
) -> Result<Option<T>, Error> {
    match Datagrams::of(mrb, value) {
        Ok(datagrams) => datagrams.with(mrb, |open| call(&open.socket)).map(Some),
        Err(_) => Ok(None),
    }
}

/// The `UDPSocket` `value` stands for, as a socket a read takes bytes from.
pub(super) fn source_of<'a>(mrb: &Mrb, value: Value) -> Result<&'a dyn SocketIo, Error> {
    Ok(Datagrams::of(mrb, value)?)
}

impl Datagrams {
    fn of<'a>(mrb: &Mrb, value: Value) -> Result<&'a Datagrams, Error> {
        <&Datagrams>::try_convert(value, mrb)
    }

    fn with<T>(
        &self,
        mrb: &Mrb,
        call: impl FnOnce(&mut OpenSocket) -> Result<T, Error>,
    ) -> Result<T, Error> {
        match self.open.borrow_mut().as_mut() {
            Some(open) => call(open),
            None => Err(errno::io_error(mrb, "closed stream")),
        }
    }
}

pub(super) fn init(mrb: &Mrb, ip_socket: RClass) -> Result<(), Error> {
    // A socket is an IO, as CRuby's `UDPSocket < IPSocket < BasicSocket < IO`.
    let socket = mrb.define_class(c"UDPSocket", ip_socket)?;
    Datagrams::mark_carriers(mrb)?;
    socket.define_singleton_method(mrb, c"new", beni::method!(udp_socket_s_new, -1))?;
    socket.define_singleton_method(mrb, c"open", beni::method!(udp_socket_s_new, -1))?;
    socket.define_method(mrb, c"bind", beni::method!(udp_socket_bind, 2))?;
    socket.define_method(mrb, c"connect", beni::method!(udp_socket_connect, 2))?;
    socket.define_method(mrb, c"send", beni::method!(udp_socket_send, -1))?;
    socket.define_method(mrb, c"recvfrom", beni::method!(udp_socket_recvfrom, -1))?;
    socket.define_method(
        mrb,
        c"recvfrom_nonblock",
        beni::method!(udp_socket_recvfrom_nonblock, -1),
    )?;
    Ok(())
}

/// Whether `family` names IPv4; a name no family has raises `SocketError`.
fn is_inet(mrb: &Mrb, family: Value) -> Result<bool, Error> {
    Ok(match family_of(mrb, family)? {
        Family::Number(number) => number == AF_INET,
        Family::Named(name) => name == "INET",
    })
}

/// `UDPSocket.new(family = AF_INET)` makes an unbound IPv4 socket; any other
/// family raises `Errno::EAFNOSUPPORT`, as `socket(2)` answers IPv6 here and
/// an unknown family everywhere.
fn udp_socket_s_new(mrb: &Mrb, class: RClass, args: &[Value]) -> Result<Value, Error> {
    if args.len() > 1 {
        return Err(Error::argnum(mrb, args.len(), 0, 1));
    }
    if let Some(family) = args.first() {
        if !is_inet(mrb, *family)? {
            return Err(errno::error_by_name(
                mrb,
                c"EAFNOSUPPORT",
                "socket(2) - udp",
            ));
        }
    }
    let socket = create_udp_socket(IpAddressFamily::Ipv4)
        .map_err(|code| to_error(mrb, code, UDP, "socket(2) - udp"))?;
    let datagrams = Datagrams {
        open: RefCell::new(Some(OpenSocket {
            streams: None,
            socket,
            send_shut: false,
        })),
        io: IoState::new(),
    };
    Ok(mrb.wrap_as(datagrams, class).as_value())
}

/// Raise `IOError` for a closed socket before a call looks its host up, as
/// CRuby's `bind`, `connect`, and `send` check the open file before
/// `rsock_addrinfo`.
fn ensure_open(mrb: &Mrb, self_: Value) -> Result<(), Error> {
    Datagrams::of(mrb, self_)?.with(mrb, |_| Ok(()))
}

/// `bind(host, port)` binds to the local end they name, as the host lets a
/// bind take it; a second bind raises `Errno::EINVAL`, as Linux's.
fn udp_socket_bind(mrb: &Mrb, self_: Value, host: Value, port: Value) -> Result<Value, Error> {
    ensure_open(mrb, self_)?;
    let (ip, port, call) = local_of(mrb, host, port, UDP)?;
    Datagrams::of(mrb, self_)?.with(mrb, |open| {
        bind(&open.socket, (ip, port)).map_err(|code| match code {
            ErrorCode::InvalidState => errno::error_by_name(mrb, c"EINVAL", &call),
            code => to_error(mrb, code, UDP, &call),
        })
    })?;
    Ok(0.into_value(mrb))
}

/// `connect(host, port)` streams with the peer `host` and `port` name, binding
/// first if no bind came before, as Linux's autobind does; the host answers a
/// socket bound already, by `bind` or an earlier `connect`, `invalid-state`
/// and keeps its end.
fn udp_socket_connect(mrb: &Mrb, self_: Value, host: Value, port: Value) -> Result<Value, Error> {
    ensure_open(mrb, self_)?;
    let (ip, port, call) = peer_of(mrb, host, port, &no_limit, UDP, "connect(2)")?;
    let failed = |code| to_error(mrb, code, UDP, &call);
    Datagrams::of(mrb, self_)?.with(mrb, |open| {
        match bind(&open.socket, (Ipv4Addr::UNSPECIFIED, 0)) {
            Ok(()) | Err(ErrorCode::InvalidState) => {}
            Err(code) => return Err(failed(code)),
        }
        open.streams = None;
        let (incoming, outgoing) = open
            .socket
            .stream(Some(to_ip_socket_address((ip, port))))
            .map_err(failed)?;
        open.streams = Some(Streams {
            incoming,
            outgoing,
            peer: (ip, port),
        });
        Ok(())
    })?;
    Ok(0.into_value(mrb))
}

/// The bytes of `mesg`, as CRuby's `send` converts it.
fn mesg_bytes_of(mrb: &Mrb, mesg: Value) -> Result<Vec<u8>, Error> {
    let mesg = mesg.to_r_string(mrb)?;
    // SAFETY: the bytes are copied before the next mruby call.
    Ok(unsafe { mesg.as_bytes(mrb) }.to_vec())
}

/// `send(mesg, flags[, host, port])` sends one datagram to the peer, waiting
/// for room; `host` is looked up as CRuby's `sendto` does, and its name keeps
/// the address `connect` was given, so the peer is judged by address, as
/// `wasi:sockets` compares one. One to another address raises
/// `Errno::EACCES`, and one with no peer at all `Errno::EDESTADDRREQ`, as
/// Linux's.
fn udp_socket_send(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<Value, Error> {
    let (mesg, flags, to) = match args {
        [mesg, flags] => (*mesg, *flags, None),
        [mesg, flags, host, port] => (*mesg, *flags, Some((*host, *port))),
        _ => return Err(Error::argnum(mrb, args.len(), 2, 4)),
    };
    let data = mesg_bytes_of(mrb, mesg)?;
    ensure_open(mrb, self_)?;
    // The flags read as CRuby's `NUM2INT` reads them, after the open file.
    let call = match to {
        Some(_) => "sendto(2)",
        None => "send(2)",
    };
    ensure_flags_carried(mrb, i64::try_convert(flags, mrb)?, call)?;
    // Whether the address a send names is the peer, `None` when it names none.
    let (names_peer, call) = match to {
        Some((host, port)) => {
            let (ip, number, call) = peer_of(mrb, host, port, &no_limit, UDP, "sendto(2)")?;
            let names_peer = Datagrams::of(mrb, self_)?.with(mrb, |open| {
                Ok(open
                    .streams
                    .as_ref()
                    .is_some_and(|streams| streams.peer == (ip, number)))
            })?;
            (Some(names_peer), call)
        }
        None => (None, "send(2)".to_string()),
    };
    let len = data.len();
    Datagrams::of(mrb, self_)?.with(mrb, |open| match open.streams_for_send(names_peer) {
        Ok(streams) => send_datagram(mrb, streams, data, &call, Wait::Block).map(ready_of),
        Err(name) => Err(errno::error_by_name(mrb, name, &call)),
    })?;
    Ok((len as i64).into_value(mrb))
}

/// Send `data` to the peer `streams` reach, waiting for room unless `wait`
/// does not wait.
fn send_datagram(
    mrb: &Mrb,
    streams: &Streams,
    data: Vec<u8>,
    call: &str,
    wait: Wait,
) -> Result<Poll<()>, Error> {
    let datagram = [OutgoingDatagram {
        data,
        remote_address: None,
    }];
    let pollable = streams.outgoing.subscribe();
    loop {
        let sent = match streams.outgoing.check_send() {
            Ok(0) => 0,
            Ok(_) => streams
                .outgoing
                .send(&datagram)
                .map_err(|code| to_error(mrb, code, UDP, call))?,
            Err(code) => return Err(to_error(mrb, code, UDP, call)),
        };
        if sent > 0 {
            return Ok(Poll::Ready(()));
        }
        if !wait_within(&pollable, wait.timeout()) {
            return Ok(Poll::Pending);
        }
    }
}

/// The next datagram `open` streams, cut to `maxlen`, and who sent it,
/// waiting for one unless `wait` does not wait; with no peer nothing can
/// arrive, so `call` raises `Errno::EACCES`.
fn receive_from(
    mrb: &Mrb,
    open: &mut OpenSocket,
    maxlen: usize,
    call: &str,
    wait: Wait,
) -> Result<Poll<Datagram>, Error> {
    let Some(streams) = &open.streams else {
        return Err(errno::error_by_name(mrb, c"EACCES", call));
    };
    let pollable = streams.incoming.subscribe();
    loop {
        match streams.incoming.receive(1) {
            Ok(mut received) if !received.is_empty() => {
                let datagram = received.swap_remove(0);
                let mut data = datagram.data;
                data.truncate(maxlen);
                let from = match datagram.remote_address {
                    IpSocketAddress::Ipv4(Ipv4SocketAddress {
                        port,
                        address: (a, b, c, d),
                    }) => (Ipv4Addr::new(a, b, c, d), port),
                    IpSocketAddress::Ipv6(_) => streams.peer,
                };
                return Ok(Poll::Ready((data, from)));
            }
            Ok(_) if !wait_within(&pollable, wait.timeout()) => return Ok(Poll::Pending),
            Ok(_) => {}
            Err(code) => return Err(to_error(mrb, code, UDP, call)),
        }
    }
}

/// `recvfrom(maxlen, flags = 0, outbuf = nil)` answers the next datagram, cut
/// to `maxlen`, with its sender as `[mesg, ["AF_INET", port, host, host]]`.
fn udp_socket_recvfrom(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<Value, Error> {
    receive_from_sender(mrb, self_, args, Wait::Block, true)
}

/// `recvfrom_nonblock(maxlen, flags = 0, outbuf = nil, exception: true)`
/// answers as `recvfrom` does, at once: with no datagram come yet it raises
/// `IO::EAGAINWaitReadable`, or answers `:wait_readable` with
/// `exception: false`.
fn udp_socket_recvfrom_nonblock(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<Value, Error> {
    let (args, exception) = exception_option(mrb, args)?;
    receive_from_sender(mrb, self_, args, Wait::DontWait, exception)
}

/// What `recvfrom` and `recvfrom_nonblock` answer, apart from whether they
/// wait; bytes a read held back raise `IOError`, as CRuby's.
fn receive_from_sender(
    mrb: &Mrb,
    self_: Value,
    args: &[Value],
    wait: Wait,
    exception: bool,
) -> Result<Value, Error> {
    let (maxlen, outbuf) = direct_args(
        mrb,
        open_socket_of(mrb, self_)?,
        args,
        "recvfrom for buffered IO",
    )?;
    let received = Datagrams::of(mrb, self_)?.with(mrb, |open| {
        receive_from(mrb, open, maxlen, "recvfrom(2)", wait)
    })?;
    let Poll::Ready((data, from)) = received else {
        return would_wait(mrb, Direction::Read, "recvfrom(2) would block", exception);
    };
    let mesg = mrb.str_new(&data).as_value();
    if let Some(outbuf) = outbuf {
        outbuf.funcall::<_, _, Value>(mrb, c"replace", [mesg])?;
    }
    let answer = mrb.ary_new();
    answer.push(mrb, outbuf.unwrap_or(mesg))?;
    answer.push(mrb, to_addr_array(mrb, from)?)?;
    Ok(answer.as_value())
}
