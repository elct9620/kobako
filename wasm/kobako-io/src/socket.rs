//! `TCPSocket`, and what `UDPSocket` shares with it, over WASI 0.2's
//! `wasi:sockets`, which the guest imports as a
//! core module, as wasi-libc imports preview1. A Sandbox giving the guest no
//! network answers every socket as unsupported, so the guest raises
//! `NotImplementedError`, as it does with no Mount; a refusal raises the
//! `Errno` CRuby's would.

use std::cell::{Cell, RefCell};
use std::ffi::CString;
use std::net::Ipv4Addr;

use beni::prelude::*;
use beni::scan_args::scan_args;
use beni::value::qnil;
use beni::{
    Error, Exception, ExceptionClass, Integer, IntoValue, Module, Mrb, Proc, Qtrue, RArray, RClass,
    RHash, Symbol, TryConvert, TypedData, Value,
};
use std::time::Duration;

use crate::errno;
use crate::io::argument_error;
use crate::wasi::wait_within;
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
    /// What a read took from the host past what it answered.
    buffered: RefCell<Vec<u8>>,
    sync: Cell<bool>,
}

/// A socket a read takes bytes from in order, as CRuby's read buffer stands
/// before a socket: what an earlier read held back comes first.
pub(crate) trait ByteSource {
    /// What a read took past what it answered.
    fn held(&self) -> &RefCell<Vec<u8>>;

    /// The next bytes the host brings, waiting for some; `None` at the end.
    fn receive(&self, mrb: &Mrb) -> Result<Option<Vec<u8>>, Error>;

    /// Whether a write is sent at once, as `IO#sync` reads; a socket's is.
    fn sync(&self) -> &Cell<bool>;
}

impl ByteSource for Connection {
    fn held(&self) -> &RefCell<Vec<u8>> {
        &self.buffered
    }

    fn receive(&self, mrb: &Mrb) -> Result<Option<Vec<u8>>, Error> {
        self.with(mrb, |open| loop {
            match open.input.blocking_read(READ_CHUNK as u64) {
                Ok(bytes) if bytes.is_empty() => continue,
                Ok(bytes) => return Ok(Some(bytes)),
                Err(StreamError::Closed) => return Ok(None),
                Err(err) => return Err(stream_error(mrb, err)),
            }
        })
    }

    fn sync(&self) -> &Cell<bool> {
        &self.sync
    }
}

/// The socket `value` stands for, a `TCPSocket` or a `UDPSocket`.
fn source_of<'a>(mrb: &Mrb, value: Value) -> Result<&'a dyn ByteSource, Error> {
    match Connection::of(mrb, value) {
        Ok(connection) => Ok(connection),
        Err(_) => crate::udp::source_of(mrb, value),
    }
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

/// Define `Socket`, `BasicSocket`, `IPSocket`, and `TCPSocket`, handing back
/// `IPSocket` for `UDPSocket` to descend from.
pub(crate) fn init(mrb: &Mrb, io: RClass) -> Result<RClass, Error> {
    let socket_error =
        mrb.class_object()
            .define_error(mrb, c"SocketError", mrb.exception_standard_error())?;
    let socket_class = mrb.define_class(c"Socket", mrb.class_object())?;
    socket_class.const_set(mrb, c"AF_INET", AF_INET)?;
    socket_class.const_set(mrb, c"AF_INET6", AF_INET6)?;
    let resolution = socket_class.define_error(mrb, c"ResolutionError", socket_error)?;
    resolution.define_method(mrb, c"error_code", beni::method!(resolution_error_code, 0))?;
    io.define_error(mrb, c"TimeoutError", mrb.exc_get(c"IOError")?)?;
    // A socket is an IO, as CRuby's `TCPSocket < IPSocket < BasicSocket < IO`.
    let basic = mrb.define_class(c"BasicSocket", io)?;
    define_socket_io(mrb, basic)?;
    let ip = mrb.define_class(c"IPSocket", basic)?;
    let socket = mrb.define_class(c"TCPSocket", ip)?;
    Connection::mark_carriers(mrb)?;
    socket.define_method(mrb, c"fileno", beni::method!(tcp_socket_fileno, 0))?;
    socket.define_method(mrb, c"to_i", beni::method!(tcp_socket_fileno, 0))?;
    socket.define_singleton_method(mrb, c"new", beni::method!(tcp_socket_s_new, -1))?;
    socket.define_singleton_method(mrb, c"open", beni::method!(tcp_socket_s_new, -1))?;
    socket.define_method(mrb, c"write", beni::method!(tcp_socket_write, -1))?;
    socket.define_method(
        mrb,
        c"close_write",
        beni::method!(tcp_socket_close_write, 0),
    )?;
    socket.define_method(mrb, c"close", beni::method!(tcp_socket_close, 0))?;
    socket.define_method(mrb, c"closed?", beni::method!(tcp_socket_closed_p, 0))?;
    socket.define_method(mrb, c"peeraddr", beni::method!(tcp_socket_peeraddr, -1))?;
    // `Socket` stands only for its constants and errors here; the guest makes
    // no raw socket, so it has no `new`.
    socket_class.singleton_class(mrb)?.funcall::<_, _, Value>(
        mrb,
        c"undef_method",
        [Symbol::new(mrb, "new")?.as_value()],
    )?;
    Ok(ip)
}

/// What every socket answers as an IO, as CRuby's `BasicSocket` does over its
/// one descriptor: reads through a buffer before the socket, a position, sync,
/// or listing it has none of answered as Linux answers each, and no copy,
/// since WASI dups no socket.
fn define_socket_io(mrb: &Mrb, basic: RClass) -> Result<(), Error> {
    basic.define_method(mrb, c"read", beni::method!(socket_read, -1))?;
    basic.define_method(mrb, c"readpartial", beni::method!(socket_readpartial, -1))?;
    basic.define_method(mrb, c"gets", beni::method!(socket_gets, -1))?;
    basic.define_method(mrb, c"each_line", beni::method!(socket_each_line, -1))?;
    basic.define_method(mrb, c"readlines", beni::method!(socket_readlines, -1))?;
    basic.define_method(mrb, c"eof?", beni::method!(socket_eof_p, 0))?;
    basic.define_method(mrb, c"eof", beni::method!(socket_eof_p, 0))?;
    for name in [
        c"seek", c"rewind", c"pos", c"pos=", c"tell", c"pread", c"pwrite",
    ] {
        basic.define_method(mrb, name, beni::method!(socket_unseekable, -1))?;
    }
    basic.define_method(mrb, c"fsync", beni::method!(socket_unsyncable, 0))?;
    basic.define_method(mrb, c"fdatasync", beni::method!(socket_unsyncable, 0))?;
    basic.define_method(mrb, c"advise", beni::method!(socket_advise, -1))?;
    basic.define_method(mrb, c"sync", beni::method!(socket_sync, 0))?;
    basic.define_method(mrb, c"sync=", beni::method!(socket_sync_set, 1))?;
    for name in ["dup", "clone"] {
        basic.funcall::<_, _, Value>(mrb, c"undef_method", [Symbol::new(mrb, name)?.as_value()])?;
    }
    Ok(())
}

/// Linux's address families, the numbers CRuby's `Socket` names them by.
pub(crate) const AF_INET: i64 = 2;
pub(crate) const AF_INET6: i64 = 10;

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
pub(crate) fn for_peer(call: &str, host: &str, port: impl std::fmt::Display) -> String {
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

/// Linux's `EAI_NONAME`, `EAI_AGAIN`, and `EAI_SERVICE`, with `gai_strerror`'s
/// words for each.
pub(crate) const EAI_NONAME: (i32, &str) = (-2, "Name or service not known");
pub(crate) const EAI_AGAIN: (i32, &str) = (-3, "Temporary failure in name resolution");
const EAI_SERVICE: (i32, &str) = (-8, "Servname not supported for ai_socktype");

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
/// unallowed one does, and a lookup slower than `limit` raises
/// `Errno::ETIMEDOUT`, as CRuby's `resolv_timeout` does.
fn resolve(
    mrb: &Mrb,
    name: &str,
    limit: Option<Duration>,
    class: &str,
    call: &str,
) -> Result<Ipv4Addr, Error> {
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
            Err(ErrorCode::WouldBlock) => {
                if !wait_within(&pollable, limit) {
                    return Err(errno::error_by_name(
                        mrb,
                        c"ETIMEDOUT",
                        "user specified timeout",
                    ));
                }
            }
            Err(code) => return Err(to_error(code)),
        }
    }
}

/// The peer `host` and `port` name for `call` on a `class`: the IPv4 address
/// `host` spells, or the one its name leads to within `resolv_limit`.
pub(crate) fn peer_of(
    mrb: &Mrb,
    host: Value,
    port: Value,
    resolv_limit: Option<Duration>,
    class: &str,
    call: &str,
) -> Result<(Ipv4Addr, u16, String), Error> {
    let host_text = host.to_r_string(mrb)?.to_string(mrb)?;
    let (port, port_text) = port_of(mrb, port)?;
    let call = for_peer(call, &host_text, port_text);
    let ip = match host_text.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(ip)) => ip,
        Ok(std::net::IpAddr::V6(_)) => {
            return Err(errno::error_by_name(mrb, c"EAFNOSUPPORT", &call))
        }
        Err(_) => resolve(mrb, &host_text, resolv_limit, class, &call)?,
    };
    Ok((ip, port, call))
}

/// The port `port` names, and its text, as Linux's `getaddrinfo` reads what
/// CRuby hands it: `nil` or `""` is port 0, and an Integer or a String of
/// digits keeps its low 16 bits. The guest has no services database, so any
/// other String raises `EAI_SERVICE`, as Linux without one does.
fn port_of(mrb: &Mrb, port: Value) -> Result<(u16, String), Error> {
    let text = if port.is_nil() {
        String::new()
    } else if Integer::from_value(port).is_some() {
        i64::try_convert(port, mrb)?.to_string()
    } else {
        let bytes = Vec::<u8>::from_value(port).ok_or_else(|| errno::type_error(mrb, port))?;
        String::from_utf8_lossy(&bytes).into_owned()
    };
    if text.is_empty() {
        return Ok((0, text));
    }
    let digits = text.trim_start_matches(|c: char| c.is_ascii_whitespace());
    let digits = digits.strip_prefix('+').unwrap_or(digits);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(resolution_error(mrb, EAI_SERVICE));
    }
    let number = digits.parse::<u64>().unwrap_or(u64::MAX);
    Ok((number as u16, text))
}

/// `(ip, port)` as WASI names a socket address.
pub(crate) fn to_ip_socket_address((ip, port): (Ipv4Addr, u16)) -> IpSocketAddress {
    let [a, b, c, d] = ip.octets();
    IpSocketAddress::Ipv4(Ipv4SocketAddress {
        port,
        address: (a, b, c, d),
    })
}

/// `TCPSocket.new(remote_host, remote_port, local_host = nil, local_port = nil,
/// connect_timeout: nil)` connects to the IPv4 address `remote_host` names, or
/// the one its name leads to, as the network the Sandbox gives allows. A local
/// end the script chooses is refused, as a chosen bind is, and a connect
/// slower than `connect_timeout` raises `IO::TimeoutError`, as CRuby's does.
fn tcp_socket_s_new(mrb: &Mrb, class: RClass, args: &[Value]) -> Result<Value, Error> {
    const TCP: &str = "TCPSocket";
    let mut args = args.to_vec();
    let options = args.last().copied().and_then(RHash::from_value);
    if options.is_some() {
        args.pop();
    }
    let (host, port, local) = match args.as_slice() {
        [host, port, local @ ..] if local.len() <= 2 => (*host, *port, local),
        _ => return Err(Error::argnum(mrb, args.len(), 2, 4)),
    };
    let Timeouts { resolv, connect } = timeouts_of(mrb, options)?;
    let (ip, port, call) = peer_of(mrb, host, port, resolv, TCP, "connect(2)")?;
    if local.iter().any(|end| !end.is_nil()) {
        return Err(errno::error_by_name(
            mrb,
            c"EACCES",
            &for_peer("bind(2)", &ip.to_string(), port),
        ));
    }
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
            Err(ErrorCode::WouldBlock) => {
                if !wait_within(&pollable, connect) {
                    return Err(Error::new(
                        mrb,
                        io_timeout_error(mrb)?,
                        &format!("user specified timeout for {ip} port {port}"),
                    ));
                }
            }
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
        sync: Cell::new(true),
    };
    Ok(mrb.wrap_as(connection, class).as_value())
}

/// The limits `TCPSocket.new`'s keywords set on resolving a name and on
/// connecting.
struct Timeouts {
    resolv: Option<Duration>,
    connect: Option<Duration>,
}

/// The limits `resolv_timeout:` and `connect_timeout:` set, refusing any
/// keyword CRuby's does not take. `fast_fallback:` chooses between IPv6 and
/// IPv4 attempts, so on a guest that speaks only IPv4 it changes nothing.
fn timeouts_of(mrb: &Mrb, options: Option<RHash>) -> Result<Timeouts, Error> {
    let mut timeouts = Timeouts {
        resolv: None,
        connect: None,
    };
    let Some(options) = options else {
        return Ok(timeouts);
    };
    let keys: RArray = options.funcall(mrb, c"keys", ())?;
    for i in 0..keys.len() {
        let given: Value = keys.entry(mrb, i as isize)?;
        let name = Symbol::from_value(given).and_then(|name| name.name(mrb).ok());
        let limit = match name.as_deref() {
            Some("resolv_timeout") => &mut timeouts.resolv,
            Some("connect_timeout") => &mut timeouts.connect,
            Some("fast_fallback") => continue,
            _ => {
                let name = given.inspect(mrb);
                return Err(argument_error(mrb, &format!("unknown keyword: {name}")));
            }
        };
        *limit = seconds_of(mrb, options.aref(mrb, given)?)?;
    }
    Ok(timeouts)
}

/// The Duration `sec` names, or none for `nil`.
fn seconds_of(mrb: &Mrb, sec: Value) -> Result<Option<Duration>, Error> {
    if sec.is_nil() {
        return Ok(None);
    }
    Duration::try_from_secs_f64(f64::try_convert(sec, mrb)?)
        .map(Some)
        .map_err(|_| argument_error(mrb, "time interval must not be negative"))
}

fn io_timeout_error(mrb: &Mrb) -> Result<ExceptionClass, Error> {
    mrb.class_get(c"IO")?.const_get(mrb, c"TimeoutError")
}

/// The `Errno` a stream's failure raises, by the POSIX name the host gives
/// it, as CRuby raises `Errno::ECONNRESET` from a reset socket's read.
fn stream_error(mrb: &Mrb, err: StreamError) -> Error {
    match err {
        StreamError::Closed => errno::error_by_name(mrb, c"EPIPE", "write(2)"),
        StreamError::LastOperationFailed(err) => {
            let name = err.to_debug_string();
            match CString::new(name.as_str()) {
                Ok(name) if errno::is_known(&name) => errno::error_alone(mrb, &name),
                _ => errno::io_error(mrb, &name),
            }
        }
    }
}

/// `write(*objects)` sends every byte of each, answering how many it sent.
fn tcp_socket_write(mrb: &Mrb, self_: Value, objects: &[Value]) -> Result<Value, Error> {
    let mut bytes = Vec::new();
    for object in objects {
        let data = object
            .funcall::<_, _, Value>(mrb, c"to_s", ())?
            .to_r_string(mrb)?;
        // SAFETY: the bytes are copied before the next mruby call.
        bytes.extend_from_slice(unsafe { data.as_bytes(mrb) });
    }
    Connection::of(mrb, self_)?.with(mrb, |open| {
        for chunk in bytes.chunks(WRITE_CHUNK) {
            open.output
                .blocking_write_and_flush(chunk)
                .map_err(|err| stream_error(mrb, err))?;
        }
        Ok(())
    })?;
    Ok((bytes.len() as i64).into_value(mrb))
}

/// Up to `len` bytes, what an earlier read held back first, else what the
/// host brings next, holding back what the read did not take; `None` at the
/// end.
fn read_some(mrb: &Mrb, source: &dyn ByteSource, len: usize) -> Result<Option<Vec<u8>>, Error> {
    let mut held = source.held().borrow_mut();
    if held.is_empty() {
        drop(held);
        let Some(bytes) = source.receive(mrb)? else {
            return Ok(None);
        };
        held = source.held().borrow_mut();
        *held = bytes;
    }
    let take = held.len().min(len);
    Ok(Some(held.drain(..take).collect()))
}

/// `value`, a `String` buffer a read is given, made to hold `read`, or
/// emptied when the read answers `nil`, as CRuby fills `outbuf`.
fn fill(mrb: &Mrb, outbuf: Option<Value>, read: Value) -> Result<Value, Error> {
    let Some(outbuf) = outbuf else {
        return Ok(read);
    };
    let filling = match read.is_nil() {
        true => mrb.str_new(b"").as_value(),
        false => read,
    };
    outbuf.funcall::<_, _, Value>(mrb, c"replace", [filling])?;
    Ok(match read.is_nil() {
        true => read,
        false => outbuf,
    })
}

/// `read(length = nil, outbuf = nil)` reads `length` bytes, or to the end
/// without one, into `outbuf` when given, as CRuby's `IO#read` does; `nil` at
/// the end when a length was asked for.
fn socket_read(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<Value, Error> {
    let (length, outbuf) = match args {
        [] => (None, None),
        [length] => (Some(*length), None),
        [length, outbuf] => (Some(*length), Some(*outbuf).filter(|buf| !buf.is_nil())),
        _ => return Err(Error::argnum(mrb, args.len(), 0, 2)),
    };
    let length = match length.filter(|v| !v.is_nil()) {
        Some(v) => Some(
            usize::try_from(i64::try_convert(v, mrb)?)
                .map_err(|_| argument_error(mrb, "negative length given"))?,
        ),
        None => None,
    };
    let source = source_of(mrb, self_)?;
    let mut read = Vec::new();
    while length.is_none_or(|length| read.len() < length) {
        let want = length.map_or(READ_CHUNK, |length| length - read.len());
        match read_some(mrb, source, want)? {
            Some(bytes) => read.extend(bytes),
            None => break,
        }
    }
    let answer = match read.is_empty() && length.is_some_and(|length| length > 0) {
        true => qnil().as_value(),
        false => mrb.str_new(&read).as_value(),
    };
    fill(mrb, outbuf, answer)
}

/// `readpartial(maxlen, outbuf = nil)` answers what one read brings, into
/// `outbuf` when given, raising `EOFError` at the end, as CRuby does.
fn socket_readpartial(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<Value, Error> {
    let (maxlen, outbuf) = match args {
        [maxlen] => (*maxlen, None),
        [maxlen, outbuf] => (*maxlen, Some(*outbuf).filter(|buf| !buf.is_nil())),
        _ => return Err(Error::argnum(mrb, args.len(), 1, 2)),
    };
    let maxlen = usize::try_from(i64::try_convert(maxlen, mrb)?)
        .map_err(|_| argument_error(mrb, "negative length given"))?;
    match read_some(mrb, source_of(mrb, self_)?, maxlen)? {
        Some(bytes) => fill(mrb, outbuf, mrb.str_new(&bytes).as_value()),
        None => Err(Error::new(
            mrb,
            mrb.exc_get(c"EOFError")?,
            "end of file reached",
        )),
    }
}

/// The next line, its separator kept unless `chomp`, or `None` at the end.
fn next_line(
    mrb: &Mrb,
    source: &dyn ByteSource,
    (separator, limit, chomp): &LineArgs,
) -> Result<Option<Vec<u8>>, Error> {
    let mut line = Vec::new();
    loop {
        if let Some(end) = line_end_of(&line, separator.as_deref(), *limit) {
            let rest = line.split_off(end);
            source.held().borrow_mut().splice(0..0, rest);
            break;
        }
        match read_some(mrb, source, READ_CHUNK)? {
            Some(bytes) => line.extend(bytes),
            None => break,
        }
    }
    if line.is_empty() {
        return Ok(None);
    }
    if let Some(separator) = separator.as_deref().filter(|_| *chomp) {
        if line.ends_with(separator) {
            line.truncate(line.len() - separator.len());
        }
    }
    Ok(Some(line))
}

/// `gets(sep = $/, limit = nil, chomp: false)` answers the next line, or
/// `nil` at the end.
fn socket_gets(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<Value, Error> {
    let line_args = line_args_of(mrb, args)?;
    Ok(match next_line(mrb, source_of(mrb, self_)?, &line_args)? {
        Some(line) => mrb.str_new(&line).as_value(),
        None => qnil().as_value(),
    })
}

/// `each_line(sep = $/, limit = nil, chomp: false) { |line| }` yields each
/// line to the end.
fn socket_each_line(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<Value, Error> {
    let block = scan_args::<(), (), RArray, (), (), Option<Proc>>(mrb)?
        .block
        .ok_or_else(|| argument_error(mrb, "each_line needs a block"))?;
    let line_args = line_args_of(mrb, args)?;
    let source = source_of(mrb, self_)?;
    while let Some(line) = next_line(mrb, source, &line_args)? {
        let _scope = mrb.arena_scope();
        block.call::<_, Value>(mrb, [mrb.str_new(&line).as_value()])?;
    }
    Ok(self_)
}

/// `readlines(sep = $/, limit = nil, chomp: false)` answers every line to the
/// end.
fn socket_readlines(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<Value, Error> {
    let line_args = line_args_of(mrb, args)?;
    let source = source_of(mrb, self_)?;
    let lines = mrb.ary_new();
    while let Some(line) = next_line(mrb, source, &line_args)? {
        lines.push(mrb, mrb.str_new(&line).as_value())?;
    }
    Ok(lines.as_value())
}

/// `eof?` waits for the next bytes, holding them for the next read, and
/// answers whether the end came instead, as CRuby's fills its buffer.
fn socket_eof_p(mrb: &Mrb, self_: Value) -> Result<bool, Error> {
    let source = source_of(mrb, self_)?;
    if !source.held().borrow().is_empty() {
        return Ok(false);
    }
    match source.receive(mrb)? {
        Some(bytes) => {
            *source.held().borrow_mut() = bytes;
            Ok(false)
        }
        None => Ok(true),
    }
}

/// A call a socket has no position for raises `Errno::ESPIPE`, as Linux's
/// `lseek`, `pread`, and `pwrite` answer one.
fn socket_unseekable(mrb: &Mrb, _self: Value, _args: &[Value]) -> Result<Value, Error> {
    Err(errno::error_alone(mrb, c"ESPIPE"))
}

/// A socket keeps nothing to sync, so `fsync` raises `Errno::EINVAL`, as
/// Linux's answers it.
fn socket_unsyncable(mrb: &Mrb, _self: Value) -> Result<Value, Error> {
    Err(errno::error_alone(mrb, c"EINVAL"))
}

/// `advise(advice, offset = 0, len = 0)` is taken and ignored, as Linux's
/// `fadvise` takes advice on a socket.
fn socket_advise(mrb: &Mrb, _self: Value, args: &[Value]) -> Result<Value, Error> {
    if !(1..=3).contains(&args.len()) {
        return Err(Error::argnum(mrb, args.len(), 1, 3));
    }
    Ok(qnil().as_value())
}

fn socket_sync(mrb: &Mrb, self_: Value) -> Result<bool, Error> {
    Ok(source_of(mrb, self_)?.sync().get())
}

fn socket_sync_set(mrb: &Mrb, self_: Value, sync: Value) -> Result<Value, Error> {
    source_of(mrb, self_)?.sync().set(sync.to_bool());
    Ok(sync)
}

/// What `gets` and its kin take: the separator, `None` to read to the end,
/// the limit, and whether to chomp the separator.
type LineArgs = (Option<Vec<u8>>, Option<usize>, bool);

/// `gets(sep = $/, limit = nil, chomp: false)`'s arguments; an empty
/// separator reads a paragraph, as CRuby's does.
fn line_args_of(mrb: &Mrb, args: &[Value]) -> Result<LineArgs, Error> {
    let (args, chomp) = match args.split_last() {
        Some((last, rest)) if RHash::try_convert(*last, mrb).is_ok() => {
            let options = RHash::try_convert(*last, mrb)?;
            let chomp: Value = options.aref(mrb, Symbol::new(mrb, "chomp")?.as_value())?;
            (rest, chomp.to_bool())
        }
        _ => (args, false),
    };
    let (separator, limit) = separator_and_limit_of(mrb, args)?;
    Ok((separator, limit, chomp))
}

/// `gets(sep = $/, limit = nil)`'s separator, `None` to read to the end, and
/// its limit.
fn separator_and_limit_of(
    mrb: &Mrb,
    args: &[Value],
) -> Result<(Option<Vec<u8>>, Option<usize>), Error> {
    let limit_of = |value: Value| -> Result<Option<usize>, Error> {
        if value.is_nil() {
            return Ok(None);
        }
        Ok(usize::try_from(i64::try_convert(value, mrb)?).ok())
    };
    let separator_of = |value: Value| -> Result<Option<Vec<u8>>, Error> {
        if value.is_nil() {
            return Ok(None);
        }
        let text = value.to_r_string(mrb)?;
        // SAFETY: the bytes are copied before the next mruby call.
        let bytes = unsafe { text.as_bytes(mrb) }.to_vec();
        Ok(Some(if bytes.is_empty() {
            b"\n\n".to_vec()
        } else {
            bytes
        }))
    };
    match args {
        [] => Ok((Some(b"\n".to_vec()), None)),
        [only] if Integer::from_value(*only).is_some() => {
            Ok((Some(b"\n".to_vec()), limit_of(*only)?))
        }
        [separator] => Ok((separator_of(*separator)?, None)),
        [separator, limit] => Ok((separator_of(*separator)?, limit_of(*limit)?)),
        _ => Err(Error::argnum(mrb, args.len(), 0, 2)),
    }
}

/// Where the line `line` starts ends: past its separator, or at its limit,
/// whichever comes first; `None` while neither is in it.
fn line_end_of(line: &[u8], separator: Option<&[u8]>, limit: Option<usize>) -> Option<usize> {
    let by_separator = separator.and_then(|separator| {
        line.windows(separator.len())
            .position(|window| window == separator)
            .map(|at| at + separator.len())
    });
    let by_limit = limit.filter(|limit| line.len() >= *limit);
    match (by_separator, by_limit) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (end, None) | (None, end) => end,
    }
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

/// The socket's descriptor, its preview2 handle, which preview1's calls and
/// `IO.select` reach as one fdtable holds a socket beside a file.
fn tcp_socket_fileno(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let fd = Connection::of(mrb, self_)?.with(mrb, |open| Ok(open.socket.handle()))?;
    Ok((fd as i64).into_value(mrb))
}

/// Whether `value` is a `TCPSocket` holding bytes a `gets` read past, which
/// a read answers before the socket is asked, as CRuby's read buffer is.
pub(crate) fn holds_buffered(mrb: &Mrb, value: Value) -> bool {
    source_of(mrb, value).is_ok_and(|source| !source.held().borrow().is_empty())
}

fn tcp_socket_closed_p(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    Ok(Connection::of(mrb, self_)?
        .open
        .borrow()
        .is_none()
        .into_value(mrb))
}

/// `peeraddr(reverse_lookup = nil)` answers the peer as CRuby's
/// `["AF_INET", port, host, host]`. The guest names no host, so a reverse
/// lookup asked for answers the address.
fn tcp_socket_peeraddr(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<Value, Error> {
    if let Some(flag) = args.first() {
        check_reverse_lookup(mrb, *flag)?;
    }
    let peer = Connection::of(mrb, self_)?.with(mrb, |open| Ok(open.peer))?;
    to_addr_array(mrb, peer)
}

/// Refuses a `reverse_lookup` flag CRuby's refuses: one of `nil`, `true`,
/// `false`, `:hostname`, or `:numeric` passes.
fn check_reverse_lookup(mrb: &Mrb, flag: Value) -> Result<(), Error> {
    if !flag.to_bool() || Qtrue::from_value(flag).is_some() {
        return Ok(());
    }
    let Some(symbol) = Symbol::from_value(flag) else {
        let message = format!(
            "wrong argument type {} (expected Symbol)",
            flag.classname(mrb)
        );
        return Err(Error::new(mrb, mrb.exception_type_error()?, &message));
    };
    match symbol.name(mrb)?.as_str() {
        "hostname" | "numeric" => Ok(()),
        _ => {
            let message = format!("invalid reverse_lookup flag: {}", flag.inspect(mrb));
            Err(argument_error(mrb, &message))
        }
    }
}

/// An address as CRuby's `["AF_INET", port, host, host]`.
pub(crate) fn to_addr_array(mrb: &Mrb, (ip, port): (Ipv4Addr, u16)) -> Result<Value, Error> {
    let host = mrb.str_new(ip.to_string().as_bytes()).as_value();
    let addr = mrb.ary_new();
    addr.push(mrb, mrb.str_new(b"AF_INET").as_value())?;
    addr.push(mrb, i64::from(port).into_value(mrb))?;
    addr.push(mrb, host)?;
    addr.push(mrb, host)?;
    Ok(addr.as_value())
}
