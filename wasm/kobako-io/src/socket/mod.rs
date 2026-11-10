//! `TCPSocket`, and what `UDPSocket` shares with it, over WASI 0.2's
//! `wasi:sockets`, which the guest imports as a
//! core module, as wasi-libc imports preview1. A Sandbox giving the guest no
//! network answers every socket as unsupported, so the guest raises
//! `NotImplementedError`, as it does with no Mount; a refusal raises the
//! `Errno` CRuby's would.
//!
//! Each class lives in its own file, as CRuby's `ext/socket` keeps one per
//! class; what they share, the `Socket` constants and errors, lives here, as
//! `init.c` holds it.
//!
//! A read either goes through the buffer CRuby's `io.c` keeps before a
//! socket, as `read`, `readpartial`, `read_nonblock`, `gets`, and `getc` do,
//! or past it, as `recv`, `recv_nonblock`, and `sysread` do, which refuse
//! while the buffer holds bytes. Both reach one receive per socket type, and
//! every write one send, each taking whether the call waits, as Linux carries
//! `MSG_DONTWAIT` into one `recvmsg` or `sendmsg`. A wait on one IO polls as
//! `IO.select` does.

mod address;
mod addrinfo;
mod basic;
mod buffer;
mod ip;
mod option;
mod tcp;
mod udp;

use std::ffi::CString;
use std::time::Duration;

use beni::prelude::*;
use beni::value::qnil;
use beni::{
    Error, Exception, ExceptionClass, Module, Mrb, RClass, RString, Symbol, TryConvert, Value,
};

pub(crate) use basic::waitable_handle;
pub(crate) use buffer::holds_buffered;

use crate::errno;
use crate::io::argument_error;
use crate::wasi::wasi::io::streams::StreamError;
use crate::wasi::wasi::sockets::network::{ErrorCode, IpAddressFamily};
use crate::wasi::wasi::sockets::tcp_create_socket::create_tcp_socket;

/// Define `Socket`, `BasicSocket`, `IPSocket`, `TCPSocket`, and `UDPSocket`.
pub(crate) fn init(mrb: &Mrb, io: RClass) -> Result<(), Error> {
    let socket_error =
        mrb.class_object()
            .define_error(mrb, c"SocketError", mrb.exception_standard_error())?;
    let socket_class = mrb.define_class(c"Socket", mrb.class_object())?;
    socket_class.const_set(mrb, c"AF_INET", AF_INET)?;
    socket_class.const_set(mrb, c"AF_INET6", AF_INET6)?;
    // Linux's `shutdown(2)` sides.
    socket_class.const_set(mrb, c"SHUT_RD", 0)?;
    socket_class.const_set(mrb, c"SHUT_WR", 1)?;
    socket_class.const_set(mrb, c"SHUT_RDWR", 2)?;
    let resolution = socket_class.define_error(mrb, c"ResolutionError", socket_error)?;
    resolution.define_method(mrb, c"error_code", beni::method!(resolution_error_code, 0))?;
    io.define_error(mrb, c"TimeoutError", mrb.exc_get(c"IOError")?)?;
    // A socket is an IO, as CRuby's `TCPSocket < IPSocket < BasicSocket < IO`.
    let basic = mrb.define_class(c"BasicSocket", io)?;
    basic::init(mrb, basic)?;
    option::init(mrb, socket_class, basic)?;
    address::init(mrb, socket_class)?;
    addrinfo::init(mrb, socket_class, basic)?;
    let ip = mrb.define_class(c"IPSocket", basic)?;
    ip::init(mrb, ip)?;
    tcp::init(mrb, socket_class, ip)?;
    udp::init(mrb, ip)?;
    // `Socket` stands only for its constants and errors here; the guest makes
    // no raw socket, so it has no `new`.
    socket_class.singleton_class(mrb)?.funcall::<_, _, Value>(
        mrb,
        c"undef_method",
        [Symbol::new(mrb, "new")?.as_value()],
    )?;
    Ok(())
}

/// Linux's address families, the numbers CRuby's `Socket` names them by.
const AF_INET: i64 = 2;
const AF_INET6: i64 = 10;

/// The address families CRuby's `Socket` names, each also spelled with `AF_`
/// or `PF_` before it, as `ext/socket/mkconstants.rb` lists them.
const FAMILIES: &[&str] = &[
    "ALG",
    "APPLETALK",
    "ATM",
    "AX25",
    "BLUETOOTH",
    "CAN",
    "CCITT",
    "CHAOS",
    "CNT",
    "COIP",
    "DATAKIT",
    "DEC",
    "DLI",
    "E164",
    "ECMA",
    "HYLINK",
    "IB",
    "IMPLINK",
    "INET",
    "INET6",
    "IPX",
    "ISDN",
    "ISO",
    "KCM",
    "KEY",
    "LAT",
    "LINK",
    "LLC",
    "LOCAL",
    "MAX",
    "MPLS",
    "NATM",
    "NDRV",
    "NETBIOS",
    "NETGRAPH",
    "NETLINK",
    "NS",
    "OSI",
    "PACKET",
    "PPP",
    "PPPOX",
    "PUP",
    "RDS",
    "ROUTE",
    "SIP",
    "SNA",
    "SYSTEM",
    "TIPC",
    "UNIX",
    "UNSPEC",
    "VSOCK",
    "XDP",
];

/// An address family a script names: a number, or a family CRuby names,
/// without its `AF_` or `PF_`.
enum Family {
    Number(i64),
    Named(String),
}

/// The name a Symbol or a String spells, `None` for any other value, as
/// CRuby's `ext/socket` reads a family, a level, or an option by name.
fn spelled(mrb: &Mrb, value: Value) -> Result<Option<String>, Error> {
    if let Some(symbol) = Symbol::from_value(value) {
        return symbol.name(mrb).map(Some);
    }
    match RString::from_value(value) {
        Some(text) => text.to_string(mrb).map(Some),
        None => Ok(None),
    }
}

/// The `SocketError` carrying `message`.
fn socket_error(mrb: &Mrb, message: &str) -> Error {
    match mrb
        .class_get(c"SocketError")
        .map(|class| ExceptionClass::from_value(class.as_value()))
    {
        Ok(Some(class)) => Error::new(mrb, class, message),
        Ok(None) => Error::new(mrb, mrb.exception_standard_error(), message),
        Err(err) => err,
    }
}

/// The family `value` names, as CRuby's `rsock_family_arg` reads a Symbol, a
/// String, or an Integer; a name no family has raises `SocketError`.
fn family_of(mrb: &Mrb, value: Value) -> Result<Family, Error> {
    let Some(name) = spelled(mrb, value)? else {
        return Ok(Family::Number(i64::try_convert(value, mrb)?));
    };
    let bare = name
        .strip_prefix("AF_")
        .or_else(|| name.strip_prefix("PF_"))
        .unwrap_or(&name);
    if !FAMILIES.contains(&bare) {
        return Err(socket_error(mrb, &format!("unknown socket domain: {name}")));
    }
    Ok(Family::Named(bare.to_string()))
}

/// The Duration `sec` names, or none for `nil`, as a socket's timeout
/// keywords read it.
fn seconds_of(mrb: &Mrb, sec: Value) -> Result<Option<Duration>, Error> {
    if sec.is_nil() {
        return Ok(None);
    }
    Duration::try_from_secs_f64(f64::try_convert(sec, mrb)?)
        .map(Some)
        .map_err(|_| argument_error(mrb, "time interval must not be negative"))
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
fn to_error(mrb: &Mrb, code: ErrorCode, class: &str, call: &str) -> Error {
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
const EAI_AGAIN: (i32, &str) = (-3, "Temporary failure in name resolution");
const EAI_SERVICE: (i32, &str) = (-8, "Servname not supported for ai_socktype");

/// Whether the Sandbox gives the guest the network: without it, no socket is
/// made, which answers at once and does nothing first.
pub(crate) fn network_given() -> bool {
    create_tcp_socket(IpAddressFamily::Ipv4).is_ok()
}

/// The `Socket::ResolutionError` CRuby raises for a `getaddrinfo` failure.
fn resolution_error(mrb: &Mrb, (code, words): (i32, &str)) -> Error {
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

/// The `Errno` a stream's failure raises, by the POSIX name the host gives
/// it, as CRuby raises `Errno::ECONNRESET` from a reset socket's read.
pub(crate) fn stream_error(mrb: &Mrb, err: StreamError) -> Error {
    match err {
        StreamError::Closed => errno::error_alone(mrb, c"EPIPE"),
        StreamError::LastOperationFailed(err) => {
            let name = err.to_debug_string();
            match CString::new(name.as_str()) {
                Ok(name) if errno::is_known(&name) => errno::error_alone(mrb, &name),
                _ => errno::io_error(mrb, &name),
            }
        }
    }
}

/// A socket WASI binds in two steps, `start-bind` then `finish-bind`, as its
/// TCP and UDP sockets both do.
pub(crate) trait Bind {
    fn start_bind(&self, end: (std::net::Ipv4Addr, u16)) -> Result<(), ErrorCode>;
    fn finish_bind(&self) -> Result<(), ErrorCode>;
    fn subscribe(&self) -> crate::wasi::wasi::io::poll::Pollable;
}

macro_rules! bind_through_wasi {
    ($($socket:ty),*) => {$(
        impl Bind for $socket {
            fn start_bind(&self, end: (std::net::Ipv4Addr, u16)) -> Result<(), ErrorCode> {
                <$socket>::start_bind(
                    self,
                    &crate::wasi::wasi::sockets::instance_network::instance_network(),
                    address::to_ip_socket_address(end),
                )
            }

            fn finish_bind(&self) -> Result<(), ErrorCode> {
                <$socket>::finish_bind(self)
            }

            fn subscribe(&self) -> crate::wasi::wasi::io::poll::Pollable {
                <$socket>::subscribe(self)
            }
        }
    )*};
}

bind_through_wasi!(
    crate::wasi::wasi::sockets::tcp::TcpSocket,
    crate::wasi::wasi::sockets::udp::UdpSocket
);

/// Bind `socket` to `end`, waiting while the host settles the bind, as
/// Linux's `bind(2)` answers once it took hold.
pub(crate) fn bind(socket: &impl Bind, end: (std::net::Ipv4Addr, u16)) -> Result<(), ErrorCode> {
    socket.start_bind(end)?;
    let pollable = socket.subscribe();
    loop {
        match socket.finish_bind() {
            Ok(()) => return Ok(()),
            Err(ErrorCode::WouldBlock) => pollable.block(),
            Err(code) => return Err(code),
        }
    }
}
