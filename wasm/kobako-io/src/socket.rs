//! `TCPSocket`, and what `UDPSocket` shares with it, over WASI 0.2's
//! `wasi:sockets`, which the guest imports as a
//! core module, as wasi-libc imports preview1. A Sandbox giving the guest no
//! network answers every socket as unsupported, so the guest raises
//! `NotImplementedError`, as it does with no Mount; a refusal raises the
//! `Errno` CRuby's would.

use std::cell::RefCell;
use std::net::Ipv4Addr;

use beni::prelude::*;
use beni::value::qnil;
use beni::{
    Error, Exception, ExceptionClass, IntoValue, Module, Mrb, RClass, TryConvert, TypedData, Value,
};

use crate::errno;
use crate::io::argument_error;
use crate::wasi::wasi::io::streams::{InputStream, OutputStream, StreamError};
use crate::wasi::wasi::sockets::instance_network::instance_network;
use crate::wasi::wasi::sockets::ip_name_lookup::resolve_addresses;
use crate::wasi::wasi::sockets::network::{
    ErrorCode, IpAddress, IpAddressFamily, IpSocketAddress, Ipv4SocketAddress,
};
use crate::wasi::wasi::sockets::tcp::{ShutdownType, TcpSocket};
use crate::wasi::wasi::sockets::tcp_create_socket::create_tcp_socket;

/// The most `blocking-write-and-flush` takes in one call, as WASI bounds it.
const WRITE_CHUNK: usize = 4096;

/// The most a read without a length asks of one blocking read, the size of
/// the host's socket buffer.
const READ_CHUNK: usize = 64 * 1024;

/// One connection, its streams dropped before the socket, as WASI asks of a
/// child resource.
struct Open {
    input: InputStream,
    output: OutputStream,
    socket: TcpSocket,
    peer: (Ipv4Addr, u16),
}

/// What a `TCPSocket` stands for, kept in its payload so a script's instance
/// variables cannot rewrite it; `None` once closed.
#[beni::wrap(class = "TCPSocket", name = "Kobako::TCPSocket")]
struct Connection {
    open: RefCell<Option<Open>>,
    /// What a `gets` read past the line it answered.
    buffered: RefCell<Vec<u8>>,
}

impl Connection {
    fn of<'a>(mrb: &Mrb, value: Value) -> Result<&'a Connection, Error> {
        <&Connection>::try_convert(value, mrb)
    }

    fn with<T>(&self, mrb: &Mrb, call: impl FnOnce(&Open) -> Result<T, Error>) -> Result<T, Error> {
        match self.open.borrow().as_ref() {
            Some(open) => call(open),
            None => Err(errno::io_error(mrb, "closed stream")),
        }
    }
}

pub(crate) fn init(mrb: &Mrb) -> Result<(), Error> {
    let socket_error =
        mrb.class_object()
            .define_error(mrb, c"SocketError", mrb.exception_standard_error())?;
    let resolution = mrb
        .define_class(c"Socket", mrb.class_object())?
        .define_error(mrb, c"ResolutionError", socket_error)?;
    resolution.define_method(mrb, c"error_code", beni::method!(resolution_error_code, 0))?;
    let socket = mrb.define_class(c"TCPSocket", mrb.class_object())?;
    Connection::mark_carriers(mrb)?;
    socket.define_singleton_method(mrb, c"new", beni::method!(tcp_socket_s_new, 2))?;
    socket.define_singleton_method(mrb, c"open", beni::method!(tcp_socket_s_new, 2))?;
    socket.define_method(mrb, c"write", beni::method!(tcp_socket_write, 1))?;
    socket.define_method(mrb, c"print", beni::method!(tcp_socket_write, 1))?;
    socket.define_method(mrb, c"read", beni::method!(tcp_socket_read, -1))?;
    socket.define_method(
        mrb,
        c"readpartial",
        beni::method!(tcp_socket_readpartial, 1),
    )?;
    socket.define_method(mrb, c"gets", beni::method!(tcp_socket_gets, 0))?;
    socket.define_method(
        mrb,
        c"close_write",
        beni::method!(tcp_socket_close_write, 0),
    )?;
    socket.define_method(mrb, c"close", beni::method!(tcp_socket_close, 0))?;
    socket.define_method(mrb, c"closed?", beni::method!(tcp_socket_closed_p, 0))?;
    socket.define_method(mrb, c"peeraddr", beni::method!(tcp_socket_peeraddr, 0))?;
    Ok(())
}

/// The `NotImplementedError` a `class` raises when the Sandbox gives the guest
/// no network.
fn no_network(mrb: &Mrb, class: &str) -> Error {
    errno::not_implemented(
        mrb,
        &format!("{class} is unimplemented: this Sandbox gives the guest no network"),
    )
}

/// The `Errno` a WASI error code stands for, from `call` on a `class`.
pub(crate) fn to_error(mrb: &Mrb, code: ErrorCode, class: &str, call: &str) -> Error {
    let name = match code {
        ErrorCode::NotSupported => return no_network(mrb, class),
        ErrorCode::AccessDenied => c"EACCES",
        ErrorCode::DatagramTooLarge => c"EMSGSIZE",
        ErrorCode::ConnectionRefused => c"ECONNREFUSED",
        ErrorCode::ConnectionReset => c"ECONNRESET",
        ErrorCode::ConnectionAborted => c"ECONNABORTED",
        ErrorCode::Timeout => c"ETIMEDOUT",
        ErrorCode::RemoteUnreachable => c"EHOSTUNREACH",
        ErrorCode::AddressNotBindable => c"EADDRNOTAVAIL",
        ErrorCode::NewSocketLimit => c"EMFILE",
        ErrorCode::WouldBlock => c"EAGAIN",
        ErrorCode::InvalidArgument => c"EINVAL",
        ErrorCode::OutOfMemory => c"ENOMEM",
        ErrorCode::InvalidState => c"ENOTCONN",
        _ => c"EIO",
    };
    errno::error_by_name(mrb, name, call)
}

/// The words CRuby's `Errno` carries for a call on `host` and `port`.
pub(crate) fn on_peer(call: &str, host: &str, port: u16) -> String {
    format!("{call} for \"{host}\" port {port}")
}

/// `Socket::ResolutionError#error_code`: the `getaddrinfo` code, as Linux
/// numbers it.
fn resolution_error_code(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    match Exception::from_value(self_) {
        Some(exception) => exception.ivar_get(mrb, c"@error_code"),
        None => Ok(qnil().as_value()),
    }
}

/// Linux's `EAI_NONAME` and `EAI_AGAIN`, with `gai_strerror`'s words for each.
pub(crate) const EAI_NONAME: (i32, &str) = (-2, "Name or service not known");
pub(crate) const EAI_AGAIN: (i32, &str) = (-3, "Temporary failure in name resolution");

/// Whether the Sandbox gives the guest the network: without it, no socket is
/// made, which answers at once and does nothing first.
pub(crate) fn network_given() -> bool {
    create_tcp_socket(IpAddressFamily::Ipv4).is_ok()
}

/// The `Socket::ResolutionError` CRuby raises for a `getaddrinfo` failure.
pub(crate) fn resolution_error(mrb: &Mrb, (code, words): (i32, &str)) -> Error {
    resolution_error_with(mrb, code, &format!("getaddrinfo: {words}"))
}

/// The `Socket::ResolutionError` for `code`, carrying `message` whole.
pub(crate) fn resolution_error_with(mrb: &Mrb, code: i32, message: &str) -> Error {
    let class = match mrb
        .class_get(c"Socket")
        .and_then(|socket| socket.const_get::<_, ExceptionClass>(mrb, c"ResolutionError"))
    {
        Ok(class) => class,
        Err(err) => return err,
    };
    let exception = class.new_str(mrb, mrb.str_new(message.as_bytes()));
    if let Err(err) = exception.ivar_set(mrb, c"@error_code", code) {
        return err;
    }
    Error::Exception(exception.as_value())
}

/// The IPv4 address `name` leads to, as kobako's resolver hands out a virtual
/// one; a name with no address raises `Socket::ResolutionError`, as an
/// unallowed one does.
fn resolve(mrb: &Mrb, name: &str, class: &str, call: &str) -> Result<Ipv4Addr, Error> {
    let to_error = |code| match code {
        ErrorCode::PermanentResolverFailure => no_network(mrb, class),
        ErrorCode::TemporaryResolverFailure => resolution_error(mrb, EAI_AGAIN),
        ErrorCode::NameUnresolvable | ErrorCode::InvalidArgument => {
            resolution_error(mrb, EAI_NONAME)
        }
        code => to_error(mrb, code, class, call),
    };
    let stream = resolve_addresses(&instance_network(), name).map_err(to_error)?;
    let pollable = stream.subscribe();
    loop {
        match stream.resolve_next_address() {
            Ok(Some(IpAddress::Ipv4((a, b, c, d)))) => return Ok(Ipv4Addr::new(a, b, c, d)),
            Ok(Some(IpAddress::Ipv6(_))) => {}
            Ok(None) => return Err(resolution_error(mrb, EAI_NONAME)),
            Err(ErrorCode::WouldBlock) => pollable.block(),
            Err(code) => return Err(to_error(code)),
        }
    }
}

/// The peer `host` and `port` name for `call` on a `class`: the IPv4 address
/// `host` spells, or the one its name leads to.
pub(crate) fn peer_of(
    mrb: &Mrb,
    host: Value,
    port: Value,
    class: &str,
    call: &str,
) -> Result<(Ipv4Addr, u16, String), Error> {
    let host_text = host.to_r_string(mrb)?.to_string(mrb)?;
    let port = u16::try_from(i64::try_convert(port, mrb)?)
        .map_err(|_| argument_error(mrb, "port out of range"))?;
    let call = on_peer(call, &host_text, port);
    let ip = match host_text.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(ip)) => ip,
        Ok(std::net::IpAddr::V6(_)) => {
            return Err(errno::error_by_name(mrb, c"EAFNOSUPPORT", &call))
        }
        Err(_) => resolve(mrb, &host_text, class, &call)?,
    };
    Ok((ip, port, call))
}

/// `(ip, port)` as WASI names a socket address.
pub(crate) fn to_ip_socket_address((ip, port): (Ipv4Addr, u16)) -> IpSocketAddress {
    let [a, b, c, d] = ip.octets();
    IpSocketAddress::Ipv4(Ipv4SocketAddress {
        port,
        address: (a, b, c, d),
    })
}

/// `TCPSocket.new(host, port)` connects to the IPv4 address `host` names, or
/// the one its name leads to, as the network the Sandbox gives allows.
fn tcp_socket_s_new(mrb: &Mrb, class: RClass, host: Value, port: Value) -> Result<Value, Error> {
    const TCP: &str = "TCPSocket";
    let (ip, port, call) = peer_of(mrb, host, port, TCP, "connect(2)")?;
    let failed = |code| to_error(mrb, code, TCP, &call);
    let socket = create_tcp_socket(IpAddressFamily::Ipv4).map_err(failed)?;
    let network = instance_network();
    socket
        .start_connect(&network, to_ip_socket_address((ip, port)))
        .map_err(failed)?;
    let pollable = socket.subscribe();
    let (input, output) = loop {
        match socket.finish_connect() {
            Ok(streams) => break streams,
            Err(ErrorCode::WouldBlock) => pollable.block(),
            Err(code) => return Err(failed(code)),
        }
    };
    let connection = Connection {
        open: RefCell::new(Some(Open {
            input,
            output,
            socket,
            peer: (ip, port),
        })),
        buffered: RefCell::new(Vec::new()),
    };
    Ok(mrb.wrap_as(connection, class).as_value())
}

fn stream_failed(mrb: &Mrb, err: StreamError) -> Error {
    match err {
        StreamError::Closed => errno::error_by_name(mrb, c"EPIPE", "write(2)"),
        StreamError::LastOperationFailed(err) => {
            let name = err.to_debug_string();
            errno::io_error(mrb, &name)
        }
    }
}

/// `write(string)` sends every byte, answering how many it sent.
fn tcp_socket_write(mrb: &Mrb, self_: Value, data: Value) -> Result<Value, Error> {
    let data = data
        .funcall::<_, _, Value>(mrb, c"to_s", ())?
        .to_r_string(mrb)?;
    // SAFETY: the bytes are copied before the next mruby call.
    let bytes = unsafe { data.as_bytes(mrb) }.to_vec();
    Connection::of(mrb, self_)?.with(mrb, |open| {
        for chunk in bytes.chunks(WRITE_CHUNK) {
            open.output
                .blocking_write_and_flush(chunk)
                .map_err(|err| stream_failed(mrb, err))?;
        }
        Ok(())
    })?;
    Ok((bytes.len() as i64).into_value(mrb))
}

/// What one blocking read brings, after what a `gets` held back; `None` at the
/// end.
fn read_some(mrb: &Mrb, connection: &Connection, len: usize) -> Result<Option<Vec<u8>>, Error> {
    let mut held = connection.buffered.borrow_mut();
    if !held.is_empty() {
        let take = held.len().min(len);
        return Ok(Some(held.drain(..take).collect()));
    }
    drop(held);
    connection.with(mrb, |open| loop {
        match open.input.blocking_read(len as u64) {
            Ok(bytes) if bytes.is_empty() => continue,
            Ok(bytes) => return Ok(Some(bytes)),
            Err(StreamError::Closed) => return Ok(None),
            Err(err) => return Err(stream_failed(mrb, err)),
        }
    })
}

/// `read(length = nil)` reads `length` bytes, or to the end without one, as
/// CRuby's `IO#read` does; `nil` at the end when a length was asked for.
fn tcp_socket_read(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<Value, Error> {
    let connection = Connection::of(mrb, self_)?;
    let length = match args.first().filter(|v| !v.is_nil()) {
        Some(v) => Some(
            usize::try_from(i64::try_convert(*v, mrb)?)
                .map_err(|_| argument_error(mrb, "negative length given"))?,
        ),
        None => None,
    };
    let mut read = Vec::new();
    while length.is_none_or(|length| read.len() < length) {
        let want = length.map_or(READ_CHUNK, |length| length - read.len());
        match read_some(mrb, connection, want)? {
            Some(bytes) => read.extend(bytes),
            None => break,
        }
    }
    if read.is_empty() && length.is_some_and(|length| length > 0) {
        return Ok(qnil().as_value());
    }
    Ok(mrb.str_new(&read).as_value())
}

/// `readpartial(maxlen)` answers what one read brings, raising `EOFError` at
/// the end, as CRuby does.
fn tcp_socket_readpartial(mrb: &Mrb, self_: Value, maxlen: Value) -> Result<Value, Error> {
    let maxlen = usize::try_from(i64::try_convert(maxlen, mrb)?)
        .map_err(|_| argument_error(mrb, "negative length given"))?;
    match read_some(mrb, Connection::of(mrb, self_)?, maxlen)? {
        Some(bytes) => Ok(mrb.str_new(&bytes).as_value()),
        None => Err(Error::new(
            mrb,
            mrb.exc_get(c"EOFError")?,
            "end of file reached",
        )),
    }
}

/// `gets` answers the next line, its newline kept, or `nil` at the end.
fn tcp_socket_gets(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let connection = Connection::of(mrb, self_)?;
    let mut line = Vec::new();
    while let Some(bytes) = read_some(mrb, connection, READ_CHUNK)? {
        match bytes.iter().position(|b| *b == b'\n') {
            Some(at) => {
                line.extend_from_slice(&bytes[..=at]);
                connection
                    .buffered
                    .borrow_mut()
                    .splice(0..0, bytes[at + 1..].iter().copied());
                return Ok(mrb.str_new(&line).as_value());
            }
            None => line.extend(bytes),
        }
    }
    if line.is_empty() {
        return Ok(qnil().as_value());
    }
    Ok(mrb.str_new(&line).as_value())
}

/// `close_write` sends the FIN once what was written is sent.
fn tcp_socket_close_write(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    Connection::of(mrb, self_)?.with(mrb, |open| {
        let _ = open.socket.shutdown(ShutdownType::Send);
        Ok(())
    })?;
    Ok(qnil().as_value())
}

fn tcp_socket_close(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    Connection::of(mrb, self_)?.open.borrow_mut().take();
    Ok(qnil().as_value())
}

fn tcp_socket_closed_p(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    Ok(Connection::of(mrb, self_)?
        .open
        .borrow()
        .is_none()
        .into_value(mrb))
}

/// `peeraddr` answers the peer as CRuby's `["AF_INET", port, host, host]`.
fn tcp_socket_peeraddr(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let peer = Connection::of(mrb, self_)?.with(mrb, |open| Ok(open.peer))?;
    addr_array(mrb, peer)
}

/// An address as CRuby's `["AF_INET", port, host, host]`.
pub(crate) fn addr_array(mrb: &Mrb, (ip, port): (Ipv4Addr, u16)) -> Result<Value, Error> {
    let host = mrb.str_new(ip.to_string().as_bytes()).as_value();
    let addr = mrb.ary_new();
    addr.push(mrb, mrb.str_new(b"AF_INET").as_value())?;
    addr.push(mrb, i64::from(port).into_value(mrb))?;
    addr.push(mrb, host)?;
    addr.push(mrb, host)?;
    Ok(addr.as_value())
}
