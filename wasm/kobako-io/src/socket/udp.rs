//! `UDPSocket` over `wasi:sockets/udp`. The guest only sends to a peer it
//! names: a socket binds to any address on an ephemeral port, then streams
//! with the one peer a `connect` names, as Linux's connected UDP socket does.

use std::cell::RefCell;
use std::ffi::CStr;
use std::net::{IpAddr, Ipv4Addr, Shutdown};
use std::task::Poll;

use beni::prelude::*;
use beni::{Error, IntoValue, Mrb, RClass, TryConvert, TypedData, Value};

use super::address::{end_of, local_of, no_timeout, peer_of, to_ip_socket_address};
use super::addrinfo::{IPPROTO_UDP, SOCK_DGRAM};
use super::basic::{
    exception_option_of, ready_of, receive_past_buffer, send_args_of, IoState, PastBuffer,
    Readiness, SocketIo, Wait,
};
use super::ip::local_end_of;
use super::option::{set_udp_option, udp_option, Refusal};
use super::{bind, family_of, to_error, Family, AF_INET};
use crate::errno;
use crate::io::Direction;
use crate::wasi::wait_within;
use crate::wasi::wasi::sockets::network::{
    ErrorCode, IpAddressFamily, IpSocketAddress, Ipv4SocketAddress,
};
use crate::wasi::wasi::sockets::udp::{
    IncomingDatagramStream, OutgoingDatagram, OutgoingDatagramStream, UdpSocket,
};
use crate::wasi::wasi::sockets::udp_create_socket::create_udp_socket;

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

    /// The next datagram, cut to `max`, and the peer that sent it.
    fn receive_from(
        &self,
        mrb: &Mrb,
        max: usize,
        wait: Wait,
        call: &str,
    ) -> Result<Poll<Option<(Vec<u8>, Option<(Ipv4Addr, u16)>)>>, Error> {
        let received = self.with(mrb, |open| receive_from(mrb, open, max, call, wait))?;
        Ok(received.map(|(data, from)| Some((data, Some(from)))))
    }

    /// `bytes` as one datagram to the peer, as `write(2)` on a connected
    /// datagram socket sends; with no peer it raises `Errno::EDESTADDRREQ`,
    /// carrying no call, as CRuby's `IO#write` does. `UDPSocket#send` answers
    /// on its own, so nothing names a call here.
    fn send(
        &self,
        mrb: &Mrb,
        bytes: &[u8],
        wait: Wait,
        _call: Option<&str>,
    ) -> Result<Poll<usize>, Error> {
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
        local_end_of(mrb, local)
    }

    fn peer_end(&self, mrb: &Mrb) -> Result<(Ipv4Addr, u16), Error> {
        self.with(mrb, |open| match &open.streams {
            Some(streams) => Ok(streams.peer),
            None => Err(errno::error_by_name(mrb, c"ENOTCONN", "getpeername(2)")),
        })
    }

    fn option(&self, mrb: &Mrb, option: (i32, i32)) -> Result<Result<i32, Refusal>, Error> {
        self.with(mrb, |open| Ok(udp_option(&open.socket, option)))
    }

    fn set_option(
        &self,
        mrb: &Mrb,
        option: (i32, i32),
        int: i32,
    ) -> Result<Result<(), Refusal>, Error> {
        self.with(mrb, |open| Ok(set_udp_option(&open.socket, option, int)))
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
    socket.define_method(mrb, c"bind", beni::method!(udp_socket_bind, 2))?;
    socket.define_method(mrb, c"connect", beni::method!(udp_socket_connect, 2))?;
    socket.define_method(mrb, c"send", beni::method!(udp_socket_send, -1))?;
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
        .map_err(|code| to_error(mrb, code, "socket(2) - udp"))?;
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
    let (ip, port, call) = local_of(mrb, host, port)?;
    Datagrams::of(mrb, self_)?.with(mrb, |open| {
        bind(&open.socket, (ip, port)).map_err(|code| match code {
            ErrorCode::InvalidState => errno::error_by_name(mrb, c"EINVAL", &call),
            code => to_error(mrb, code, &call),
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
    let (ip, port, call) = peer_of(mrb, host, port, &no_timeout, "connect(2)")?;
    let failed = |code| to_error(mrb, code, &call);
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

/// `send(mesg, flags[, dest | host, port])` sends one datagram to the peer,
/// waiting for room; `dest`, a `sockaddr` String or an `Addrinfo`, names an
/// address as CRuby's `sendto` takes one, and `host` is looked up, its name
/// keeping the address `connect` was given, so the peer is judged by
/// address, as `wasi:sockets` compares one. One to another address raises
/// `Errno::EACCES`, and one with no peer at all `Errno::EDESTADDRREQ`, as
/// Linux's.
fn udp_socket_send(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<Value, Error> {
    let (mesg, flags, dest, host_port) = match args {
        [mesg, flags] => (*mesg, *flags, None, None),
        [mesg, flags, dest] => (*mesg, *flags, Some(*dest), None),
        [mesg, flags, host, port] => (*mesg, *flags, None, Some((*host, *port))),
        _ => return Err(Error::argnum(mrb, args.len(), 2, 4)),
    };
    let send = send_args_of(mrb, self_, mesg, flags, dest, host_port.is_some())?;
    let (data, call) = (send.bytes, send.call);
    // The address a send names, and the words its `Errno` carries.
    let addressed = match (send.dest, host_port) {
        (Some(sockaddr), _) => match end_of(mrb, sockaddr)? {
            (IpAddr::V4(ip), port) => Some((ip, port, call.to_string())),
            (IpAddr::V6(_), _) => return Err(errno::error_by_name(mrb, c"EAFNOSUPPORT", call)),
        },
        (None, Some((host, port))) => Some(peer_of(mrb, host, port, &no_timeout, call)?),
        (None, None) => None,
    };
    // Whether the address a send names is the peer, `None` when it names none.
    let (names_peer, call) = match addressed {
        Some((ip, number, call)) => {
            let names_peer = Datagrams::of(mrb, self_)?.with(mrb, |open| {
                Ok(open
                    .streams
                    .as_ref()
                    .is_some_and(|streams| streams.peer == (ip, number)))
            })?;
            (Some(names_peer), call)
        }
        None => (None, call.to_string()),
    };
    let len = data.len();
    Datagrams::of(mrb, self_)?.with(mrb, |open| match open.streams_for_send(names_peer) {
        Ok(streams) => send_datagram(mrb, streams, data, &call, Wait::Blocking).map(ready_of),
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
                .map_err(|code| to_error(mrb, code, call))?,
            Err(code) => return Err(to_error(mrb, code, call)),
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
            Err(code) => return Err(to_error(mrb, code, call)),
        }
    }
}

/// `recvfrom_nonblock(maxlen, flags = 0, outbuf = nil, exception: true)`
/// answers as `recvfrom` does, at once: with no datagram come yet it raises
/// `IO::EAGAINWaitReadable`, or answers `:wait_readable` with
/// `exception: false`.
fn udp_socket_recvfrom_nonblock(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<Value, Error> {
    let (args, exception) = exception_option_of(mrb, args)?;
    let past = PastBuffer::new(Wait::DontWait, "recvfrom", true).with_exception(exception);
    receive_past_buffer(mrb, self_, args, past)
}
