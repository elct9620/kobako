//! `TCPSocket`, and what `UDPSocket` shares with it, over WASI 0.2's
//! `wasi:sockets`, which the guest imports as a core module, as wasi-libc
//! imports preview1. A Sandbox giving the guest no network denies every
//! socket, as a WASI host with sockets disallowed does; that and every other
//! refusal raise the `Errno` CRuby's would.
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

use beni::prelude::*;
use beni::value::qnil;
use beni::{
    Error, Exception, ExceptionClass, IntoValue, Module, Mrb, RClass, RModule, Symbol, Value,
};

pub(crate) use address::port_of;
use address::{NI_MAXHOST, NI_MAXSERV};
pub(crate) use basic::{is_open_socket, readiness_of, select_readiness_of, Readiness};
pub(crate) use buffer::holds_buffered;

use crate::errno;
use crate::wasi::wasi::io::streams::StreamError;
use crate::wasi::wasi::sockets::network::{network_error_code, ErrorCode};

/// Define `Socket`, `BasicSocket`, `IPSocket`, `TCPSocket`, and `UDPSocket`.
pub(crate) fn init(mrb: &Mrb, io: RClass) -> Result<(), Error> {
    let socket_error =
        mrb.class_object()
            .define_error(mrb, c"SocketError", mrb.exception_standard_error())?;
    let socket_class = mrb.define_class(c"Socket", mrb.class_object())?;
    let constants = Constants {
        socket: socket_class,
        module: socket_class.define_module(mrb, c"Constants")?,
    };
    constants.set_all(mrb, SOCKET_CONSTANTS)?;
    let resolution = socket_class.define_error(mrb, c"ResolutionError", socket_error)?;
    resolution.define_method(mrb, c"error_code", beni::method!(resolution_error_code, 0))?;
    io.define_error(mrb, c"TimeoutError", mrb.exc_get(c"IOError")?)?;
    // A socket is an IO, as CRuby's `TCPSocket < IPSocket < BasicSocket < IO`.
    let basic = mrb.define_class(c"BasicSocket", io)?;
    basic::init(mrb, basic)?;
    option::init(mrb, &constants, basic)?;
    address::init(mrb, socket_class)?;
    addrinfo::init(mrb, &constants, basic)?;
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

/// Where a `Socket` constant is defined: on `Socket` and on
/// `Socket::Constants` alike, as CRuby's `rsock_init_socket_constants`
/// defines each in both.
pub(super) struct Constants {
    socket: RClass,
    module: RModule,
}

impl Constants {
    pub(super) fn set(
        &self,
        mrb: &Mrb,
        name: &str,
        value: impl IntoValue + Copy,
    ) -> Result<(), Error> {
        self.socket.const_set(mrb, name, value)?;
        self.module.const_set(mrb, name, value)
    }

    pub(super) fn set_all(&self, mrb: &Mrb, constants: &[(&str, i64)]) -> Result<(), Error> {
        constants
            .iter()
            .try_for_each(|(name, value)| self.set(mrb, name, *value))
    }
}

/// The constants every socket class shares, at Linux's numbers: the IP
/// families, `shutdown(2)`'s sides, the one message flag the guest takes,
/// the IPv4 addresses CRuby names, and `getaddrinfo`'s and
/// `getnameinfo`'s codes and sizes.
const SOCKET_CONSTANTS: &[(&str, i64)] = &[
    ("AF_UNSPEC", 0),
    ("PF_UNSPEC", 0),
    ("AF_INET", AF_INET),
    ("PF_INET", AF_INET),
    ("AF_INET6", AF_INET6),
    ("PF_INET6", AF_INET6),
    ("SHUT_RD", basic::SHUT_RD),
    ("SHUT_WR", basic::SHUT_WR),
    ("SHUT_RDWR", basic::SHUT_RDWR),
    ("MSG_DONTWAIT", basic::MSG_DONTWAIT),
    ("INADDR_ANY", 0),
    ("INADDR_LOOPBACK", 0x7f00_0001),
    ("INADDR_BROADCAST", 0xffff_ffff),
    ("INADDR_NONE", 0xffff_ffff),
    ("EAI_BADFLAGS", EAI_BADFLAGS.0 as i64),
    ("EAI_NONAME", EAI_NONAME.0 as i64),
    ("EAI_AGAIN", EAI_AGAIN.0 as i64),
    ("EAI_FAIL", EAI_FAIL.0 as i64),
    ("EAI_FAMILY", EAI_FAMILY.0 as i64),
    ("EAI_SOCKTYPE", EAI_SOCKTYPE.0 as i64),
    ("EAI_SERVICE", EAI_SERVICE.0 as i64),
    ("EAI_ADDRFAMILY", EAI_ADDRFAMILY.0 as i64),
    ("EAI_MEMORY", -10),
    ("EAI_SYSTEM", -11),
    ("EAI_OVERFLOW", -12),
    ("NI_MAXHOST", NI_MAXHOST as i64),
    ("NI_MAXSERV", NI_MAXSERV as i64),
];

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

impl Family {
    /// The number of the IP family it names, `AF_INET` or `AF_INET6`, the
    /// two the guest numbers; `None` for any other.
    fn ip_number(&self) -> Option<i64> {
        match self {
            Family::Number(number) => Some(*number),
            Family::Named(name) => match name.as_str() {
                "INET" => Some(AF_INET),
                "INET6" => Some(AF_INET6),
                _ => None,
            },
        }
        .filter(|number| [AF_INET, AF_INET6].contains(number))
    }
}

/// The name a Symbol or a String spells, `None` for any other value, as
/// CRuby's `constant_arg` reads a family, a level, or an option by name
/// through `rb_check_string_type`.
fn name_of(mrb: &Mrb, value: Value) -> Result<Option<String>, Error> {
    if let Some(symbol) = Symbol::from_value(value) {
        return symbol.name(mrb).map(Some);
    }
    match crate::convert::check_string_type(mrb, value)? {
        Some(text) => text.to_string(mrb).map(Some),
        None => Ok(None),
    }
}

/// The `SocketError` carrying `message`.
fn socket_error(mrb: &Mrb, message: &str) -> Error {
    errno::to_error(mrb, mrb.exc_get(c"SocketError"), message)
}

/// A family's name without its `AF_` or `PF_`, as CRuby's
/// `rsock_family_to_int` reads either spelling.
fn bare_family_of(name: &str) -> &str {
    name.strip_prefix("AF_")
        .or_else(|| name.strip_prefix("PF_"))
        .unwrap_or(name)
}

/// The family `value` names, as CRuby's `rsock_family_arg` reads a Symbol, a
/// String, or an Integer; a name no family has raises `SocketError`.
fn family_of(mrb: &Mrb, value: Value) -> Result<Family, Error> {
    let Some(name) = name_of(mrb, value)? else {
        return Ok(Family::Number(crate::convert::num2int(mrb, value)?));
    };
    let bare = bare_family_of(&name);
    if !FAMILIES.contains(&bare) {
        return Err(socket_error(mrb, &format!("unknown socket domain: {name}")));
    }
    Ok(Family::Named(bare.to_string()))
}

/// The `Errno` a WASI error code stands for, from `call`, as wasi-libc's
/// `__wasilibc_map_socket_error` maps each.
fn to_error(mrb: &Mrb, code: ErrorCode, call: &str) -> Error {
    errno::error_by_name(mrb, name_of_code(code), call)
}

/// The POSIX name of the `Errno` a WASI error code stands for.
fn name_of_code(code: ErrorCode) -> &'static core::ffi::CStr {
    match code {
        ErrorCode::NotSupported => c"ENOTSUP",
        ErrorCode::AccessDenied => c"EACCES",
        ErrorCode::DatagramTooLarge => c"EMSGSIZE",
        ErrorCode::ConnectionRefused => c"ECONNREFUSED",
        ErrorCode::ConnectionReset => c"ECONNRESET",
        ErrorCode::ConnectionAborted => c"ECONNABORTED",
        ErrorCode::Timeout => c"ETIMEDOUT",
        ErrorCode::RemoteUnreachable => c"EHOSTUNREACH",
        ErrorCode::AddressNotBindable => c"EADDRNOTAVAIL",
        ErrorCode::AddressInUse => c"EADDRINUSE",
        ErrorCode::NewSocketLimit => c"EMFILE",
        ErrorCode::WouldBlock => c"EAGAIN",
        ErrorCode::InvalidArgument => c"EINVAL",
        ErrorCode::OutOfMemory => c"ENOMEM",
        ErrorCode::InvalidState | ErrorCode::NotInProgress => c"ENOTCONN",
        ErrorCode::ConcurrencyConflict => c"EALREADY",
        _ => c"EOPNOTSUPP",
    }
}

/// The `Errno` a stream's failure raises, by the error code
/// `network-error-code` downcasts it to, as a socket call's code maps; the
/// stream's end, or a failure no code names, raises as any stream's does.
pub(crate) fn from_stream(mrb: &Mrb, err: StreamError, call: Option<&str>) -> Error {
    match err {
        StreamError::LastOperationFailed(failure) => match network_error_code(&failure) {
            Some(code) => errno::error_in(mrb, name_of_code(code), call),
            None => errno::from_stream(mrb, StreamError::LastOperationFailed(failure), call),
        },
        err => errno::from_stream(mrb, err, call),
    }
}

/// `Socket::ResolutionError#error_code`: the `getaddrinfo` code, as Linux
/// numbers it.
fn resolution_error_code(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    match Exception::from_value(self_) {
        Some(exception) => exception.ivar_get(mrb, c"@error_code"),
        None => Ok(qnil().as_value()),
    }
}

/// glibc's `getaddrinfo` codes on Linux, with `gai_strerror`'s words for
/// each.
const EAI_BADFLAGS: (i32, &str) = (-1, "Bad value for ai_flags");
pub(crate) const EAI_NONAME: (i32, &str) = (-2, "Name or service not known");
const EAI_AGAIN: (i32, &str) = (-3, "Temporary failure in name resolution");
const EAI_FAIL: (i32, &str) = (-4, "Non-recoverable failure in name resolution");
const EAI_FAMILY: (i32, &str) = (-6, "ai_family not supported");
const EAI_SOCKTYPE: (i32, &str) = (-7, "ai_socktype not supported");
const EAI_SERVICE: (i32, &str) = (-8, "Servname not supported for ai_socktype");
const EAI_ADDRFAMILY: (i32, &str) = (-9, "Address family for hostname not supported");

/// The `Socket::ResolutionError` CRuby raises for a `getaddrinfo` failure.
fn resolution_error(mrb: &Mrb, (code, words): (i32, &str)) -> Error {
    to_resolution_error(mrb, code, &format!("getaddrinfo: {words}"))
}

/// The `Socket::ResolutionError` for `code`, carrying `message` whole.
pub(crate) fn to_resolution_error(mrb: &Mrb, code: i32, message: &str) -> Error {
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
