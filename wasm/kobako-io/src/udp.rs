//! `UDPSocket` over `wasi:sockets/udp`. The guest only sends to a peer it
//! names: a socket binds to any address on an ephemeral port, then streams
//! with the one peer a `connect` names, as Linux's connected UDP socket does.

use std::cell::{Cell, RefCell};
use std::net::Ipv4Addr;

use beni::prelude::*;
use beni::value::qnil;
use beni::{Error, IntoValue, Mrb, RClass, TryConvert, TypedData, Value};

use crate::errno;
use crate::io::argument_error;
use crate::socket::{
    ensure_reverse_lookup_flag, for_peer, local_addr_array, peer_of, to_addr_array, to_error,
    to_ip_socket_address, ByteSource, AF_INET,
};
use crate::wasi::wasi::sockets::instance_network::instance_network;
use crate::wasi::wasi::sockets::network::{
    ErrorCode, IpAddressFamily, IpSocketAddress, Ipv4SocketAddress,
};
use crate::wasi::wasi::sockets::udp::{
    IncomingDatagramStream, OutgoingDatagram, OutgoingDatagramStream, UdpSocket,
};
use crate::wasi::wasi::sockets::udp_create_socket::create_udp_socket;

const UDP: &str = "UDPSocket";

/// The most one IPv4 datagram carries, so a read of one takes it whole.
const MAX_DATAGRAM: usize = 65_507;

/// The streams with one peer, dropped before their socket, as WASI asks of a
/// child resource.
struct Streams {
    incoming: IncomingDatagramStream,
    outgoing: OutgoingDatagramStream,
    peer: (Ipv4Addr, u16),
}

struct Open {
    streams: Option<Streams>,
    socket: UdpSocket,
    bound: bool,
}

/// What a `UDPSocket` stands for, kept in its payload so a script's instance
/// variables cannot rewrite it; `None` once closed.
#[beni::wrap(class = "UDPSocket", name = "Kobako::UDPSocket")]
struct Datagrams {
    open: RefCell<Option<Open>>,
    /// What a read took of a datagram past what it answered.
    held: RefCell<Vec<u8>>,
    sync: Cell<bool>,
}

impl ByteSource for Datagrams {
    fn held(&self) -> &RefCell<Vec<u8>> {
        &self.held
    }

    /// The next datagram whole, as a datagram socket's read fills CRuby's
    /// buffer with one.
    fn receive(&self, mrb: &Mrb) -> Result<Option<Vec<u8>>, Error> {
        self.with(mrb, |open| receive_from(mrb, open, MAX_DATAGRAM, "read(2)"))
            .map(|(data, _)| Some(data))
    }

    fn sync(&self) -> &Cell<bool> {
        &self.sync
    }
}

/// The `UDPSocket` `value` stands for, as a socket a read takes bytes from.
pub(crate) fn source_of<'a>(mrb: &Mrb, value: Value) -> Result<&'a dyn ByteSource, Error> {
    Ok(Datagrams::of(mrb, value)?)
}

impl Datagrams {
    fn of<'a>(mrb: &Mrb, value: Value) -> Result<&'a Datagrams, Error> {
        <&Datagrams>::try_convert(value, mrb)
    }

    fn with<T>(
        &self,
        mrb: &Mrb,
        call: impl FnOnce(&mut Open) -> Result<T, Error>,
    ) -> Result<T, Error> {
        match self.open.borrow_mut().as_mut() {
            Some(open) => call(open),
            None => Err(errno::io_error(mrb, "closed stream")),
        }
    }
}

pub(crate) fn init(mrb: &Mrb, ip_socket: RClass) -> Result<(), Error> {
    // A socket is an IO, as CRuby's `UDPSocket < IPSocket < BasicSocket < IO`.
    let socket = mrb.define_class(c"UDPSocket", ip_socket)?;
    Datagrams::mark_carriers(mrb)?;
    socket.define_method(mrb, c"fileno", beni::method!(udp_socket_fileno, 0))?;
    socket.define_method(mrb, c"to_i", beni::method!(udp_socket_fileno, 0))?;
    socket.define_method(mrb, c"write", beni::method!(udp_socket_write, -1))?;
    socket.define_singleton_method(mrb, c"new", beni::method!(udp_socket_s_new, -1))?;
    socket.define_singleton_method(mrb, c"open", beni::method!(udp_socket_s_new, -1))?;
    socket.define_method(mrb, c"bind", beni::method!(udp_socket_bind, 2))?;
    socket.define_method(mrb, c"connect", beni::method!(udp_socket_connect, 2))?;
    socket.define_method(mrb, c"send", beni::method!(udp_socket_send, -1))?;
    socket.define_method(mrb, c"recv", beni::method!(udp_socket_recv, -1))?;
    socket.define_method(mrb, c"recvfrom", beni::method!(udp_socket_recvfrom, -1))?;
    socket.define_method(mrb, c"close", beni::method!(udp_socket_close, 0))?;
    socket.define_method(mrb, c"closed?", beni::method!(udp_socket_closed_p, 0))?;
    socket.define_method(mrb, c"addr", beni::method!(udp_socket_addr, -1))?;
    Ok(())
}

/// Whether `family` names IPv4, as CRuby spells it by number or by name.
fn is_inet(mrb: &Mrb, family: Value) -> Result<bool, Error> {
    if let Ok(number) = i64::try_convert(family, mrb) {
        return Ok(number == AF_INET);
    }
    let name = family.to_r_string(mrb)?.to_string(mrb)?;
    Ok(matches!(name.as_str(), "INET" | "AF_INET"))
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
        open: RefCell::new(Some(Open {
            streams: None,
            socket,
            bound: false,
        })),
        held: RefCell::new(Vec::new()),
        sync: Cell::new(true),
    };
    Ok(mrb.wrap_as(datagrams, class).as_value())
}

/// Bind to any address on an ephemeral port, the one bind the guest makes.
fn bind_any(open: &mut Open, port: u16) -> Result<(), ErrorCode> {
    if open.bound {
        return Err(ErrorCode::InvalidState);
    }
    let any = to_ip_socket_address((Ipv4Addr::UNSPECIFIED, port));
    open.socket.start_bind(&instance_network(), any)?;
    let pollable = open.socket.subscribe();
    loop {
        match open.socket.finish_bind() {
            Ok(()) => break,
            Err(ErrorCode::WouldBlock) => pollable.block(),
            Err(code) => return Err(code),
        }
    }
    open.bound = true;
    Ok(())
}

/// `bind(host, port)` binds to any address on port `0`; a chosen address or
/// port raises `Errno::EACCES`, and a second bind `Errno::EINVAL`, as Linux's.
fn udp_socket_bind(mrb: &Mrb, self_: Value, host: Value, port: Value) -> Result<Value, Error> {
    let host = host.to_r_string(mrb)?.to_string(mrb)?;
    let port = u16::try_from(i64::try_convert(port, mrb)?)
        .map_err(|_| argument_error(mrb, "port out of range"))?;
    let call = for_peer("bind(2)", &host, port);
    let any = host.is_empty() || host.parse::<Ipv4Addr>() == Ok(Ipv4Addr::UNSPECIFIED);
    Datagrams::of(mrb, self_)?.with(mrb, |open| {
        if !any {
            return Err(errno::error_by_name(mrb, c"EACCES", &call));
        }
        bind_any(open, port).map_err(|code| match code {
            ErrorCode::InvalidState => errno::error_by_name(mrb, c"EINVAL", &call),
            code => to_error(mrb, code, UDP, &call),
        })
    })?;
    Ok(0.into_value(mrb))
}

/// `connect(host, port)` streams with the peer `host` and `port` name, binding
/// first if no bind came before, as Linux's autobind does.
fn udp_socket_connect(mrb: &Mrb, self_: Value, host: Value, port: Value) -> Result<Value, Error> {
    let (ip, port, call) = peer_of(mrb, host, port, None, UDP, "connect(2)")?;
    let failed = |code| to_error(mrb, code, UDP, &call);
    Datagrams::of(mrb, self_)?.with(mrb, |open| {
        if !open.bound {
            bind_any(open, 0).map_err(failed)?;
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
fn bytes_of(mrb: &Mrb, mesg: Value) -> Result<Vec<u8>, Error> {
    let mesg = mesg.to_r_string(mrb)?;
    // SAFETY: the bytes are copied before the next mruby call.
    Ok(unsafe { mesg.as_bytes(mrb) }.to_vec())
}

/// `send(mesg, flags[, host, port])` sends one datagram to the peer, waiting
/// for room; one to an address no `connect` named raises `Errno::EACCES`, and
/// one with no peer at all `Errno::EDESTADDRREQ`, as Linux's.
fn udp_socket_send(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<Value, Error> {
    let (mesg, to) = match args {
        [mesg, _flags] => (*mesg, None),
        [mesg, _flags, host, port] => (*mesg, Some((*host, *port))),
        _ => return Err(Error::argnum(mrb, args.len(), 2, 4)),
    };
    let data = bytes_of(mrb, mesg)?;
    let (remote_address, call) = match to {
        Some((host, port)) => {
            let (ip, port, call) = peer_of(mrb, host, port, None, UDP, "sendto(2)")?;
            (Some(to_ip_socket_address((ip, port))), call)
        }
        None => (None, "send(2)".to_string()),
    };
    let len = data.len();
    Datagrams::of(mrb, self_)?.with(mrb, |open| {
        let Some(streams) = &open.streams else {
            let name = if remote_address.is_some() {
                c"EACCES"
            } else {
                c"EDESTADDRREQ"
            };
            return Err(errno::error_by_name(mrb, name, &call));
        };
        send_datagram(mrb, streams, data, remote_address, &call)
    })?;
    Ok((len as i64).into_value(mrb))
}

/// `write(*objects)` sends what the objects spell as one datagram to the peer,
/// as `write(2)` on a connected datagram socket does; with no peer it raises
/// `Errno::EDESTADDRREQ`, carrying no call, as CRuby's `IO#write` does.
fn udp_socket_write(mrb: &Mrb, self_: Value, objects: &[Value]) -> Result<Value, Error> {
    let mut data = Vec::new();
    for object in objects {
        data.extend(bytes_of(
            mrb,
            object.funcall::<_, _, Value>(mrb, c"to_s", ())?,
        )?);
    }
    let len = data.len();
    Datagrams::of(mrb, self_)?.with(mrb, |open| match &open.streams {
        Some(streams) => send_datagram(mrb, streams, data, None, "write(2)"),
        None => Err(errno::error_alone(mrb, c"EDESTADDRREQ")),
    })?;
    Ok((len as i64).into_value(mrb))
}

/// Send `data` to the peer `streams` reach, waiting for room.
fn send_datagram(
    mrb: &Mrb,
    streams: &Streams,
    data: Vec<u8>,
    remote_address: Option<IpSocketAddress>,
    call: &str,
) -> Result<(), Error> {
    let datagram = [OutgoingDatagram {
        data,
        remote_address,
    }];
    let pollable = streams.outgoing.subscribe();
    loop {
        match streams.outgoing.check_send() {
            Ok(0) => pollable.block(),
            Ok(_) => match streams.outgoing.send(&datagram) {
                Ok(0) => pollable.block(),
                Ok(_) => return Ok(()),
                Err(code) => return Err(to_error(mrb, code, UDP, call)),
            },
            Err(code) => return Err(to_error(mrb, code, UDP, call)),
        }
    }
}

/// The number the socket's preview2 handle took, which `IO.select` waits on.
fn udp_socket_fileno(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let fd = Datagrams::of(mrb, self_)?.with(mrb, |open| Ok(open.socket.handle()))?;
    Ok((fd as i64).into_value(mrb))
}

/// The next datagram from the peer, cut to `maxlen`, and who sent it, waiting
/// for one; with no peer nothing can arrive, so it raises `Errno::EACCES`.
fn receive(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<(Vec<u8>, (Ipv4Addr, u16)), Error> {
    let maxlen = match args {
        [maxlen] | [maxlen, _] => usize::try_from(i64::try_convert(*maxlen, mrb)?)
            .map_err(|_| argument_error(mrb, "negative length given"))?,
        _ => return Err(Error::argnum(mrb, args.len(), 1, 2)),
    };
    Datagrams::of(mrb, self_)?.with(mrb, |open| receive_from(mrb, open, maxlen, "recvfrom(2)"))
}

/// The next datagram `open` streams, cut to `maxlen`, and who sent it,
/// waiting for one; with no peer nothing can arrive, so `call` raises
/// `Errno::EACCES`.
fn receive_from(
    mrb: &Mrb,
    open: &mut Open,
    maxlen: usize,
    call: &str,
) -> Result<(Vec<u8>, (Ipv4Addr, u16)), Error> {
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
                return Ok((data, from));
            }
            Ok(_) => pollable.block(),
            Err(code) => return Err(to_error(mrb, code, UDP, call)),
        }
    }
}

/// `recv(maxlen, flags = 0)` answers the next datagram, cut to `maxlen`.
fn udp_socket_recv(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<Value, Error> {
    let (data, _) = receive(mrb, self_, args)?;
    Ok(mrb.str_new(&data).as_value())
}

/// `recvfrom(maxlen, flags = 0)` answers `[mesg, ["AF_INET", port, host, host]]`.
fn udp_socket_recvfrom(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<Value, Error> {
    let (data, from) = receive(mrb, self_, args)?;
    let answer = mrb.ary_new();
    answer.push(mrb, mrb.str_new(&data).as_value())?;
    answer.push(mrb, to_addr_array(mrb, from)?)?;
    Ok(answer.as_value())
}

fn udp_socket_close(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    Datagrams::of(mrb, self_)?.open.borrow_mut().take();
    Ok(qnil().as_value())
}

/// `addr(reverse_lookup = nil)` answers the socket's own end as `TCPSocket`'s
/// does; one not bound yet answers the unspecified address.
fn udp_socket_addr(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<Value, Error> {
    if let Some(flag) = args.first() {
        ensure_reverse_lookup_flag(mrb, *flag)?;
    }
    let local = Datagrams::of(mrb, self_)?.with(mrb, |open| Ok(open.socket.local_address()))?;
    local_addr_array(mrb, local, UDP)
}

fn udp_socket_closed_p(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    Ok(Datagrams::of(mrb, self_)?
        .open
        .borrow()
        .is_none()
        .into_value(mrb))
}
