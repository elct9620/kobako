//! `BasicSocket#setsockopt`, `#getsockopt`, and `Socket::Option`, as CRuby's
//! `ext/socket` reads a level, a name, and a value. An option reaches the host
//! only as the WASI call that stands for it; one WASI has no call for answers
//! `Errno::ENOPROTOOPT`, as Linux answers an option it does not know.

use beni::prelude::*;
use beni::{
    Error, IntoValue, Mrb, Qfalse, Qtrue, RClass, RString, RTypedData, TryConvert, TypedData, Value,
};

use super::tcp::with_tcp_socket;
use super::udp::with_udp_socket;
use super::{family_of, name_of, socket_error, to_error, Family, AF_INET, AF_INET6};
use crate::errno;
use crate::wasi::wasi::sockets::network::ErrorCode;
use crate::wasi::wasi::sockets::tcp::TcpSocket;
use crate::wasi::wasi::sockets::udp::UdpSocket;

// Linux's numbers for the levels and options the guest names.
const IPPROTO_IP: i32 = 0;
const IP_TTL: i32 = 2;
const SOL_SOCKET: i32 = 1;
const SO_SNDBUF: i32 = 7;
const SO_RCVBUF: i32 = 8;
const SO_KEEPALIVE: i32 = 9;
const IPPROTO_TCP: i32 = 6;
const TCP_NODELAY: i32 = 1;
const TCP_KEEPIDLE: i32 = 4;
const TCP_KEEPINTVL: i32 = 5;
const TCP_KEEPCNT: i32 = 6;

/// Linux's ceilings on the keepalive options, `MAX_TCP_KEEPIDLE`,
/// `MAX_TCP_KEEPINTVL`, and `MAX_TCP_KEEPCNT`, past which it answers EINVAL.
const MAX_TCP_KEEPIDLE: u32 = 32767;
const MAX_TCP_KEEPINTVL: u32 = 32767;
const MAX_TCP_KEEPCNT: u32 = 127;

const NANOS_PER_SEC: u64 = 1_000_000_000;

/// One protocol level and the options CRuby names at it, each also spelled
/// with the level's option prefix before it.
struct Level {
    number: i32,
    name: &'static str,
    /// The prefix CRuby also accepts before the level's name.
    prefix: &'static str,
    /// The prefix each of the level's options also takes.
    option_prefix: &'static str,
    /// The words CRuby's `SocketError` names the level by.
    kind: &'static str,
    options: &'static [(&'static str, i32)],
}

/// The levels and options the guest names, at Linux's numbers.
const LEVELS: [Level; 3] = [
    Level {
        number: IPPROTO_IP,
        name: "IP",
        prefix: "IPPROTO_",
        option_prefix: "IP_",
        kind: "IP",
        options: &[("TTL", IP_TTL)],
    },
    Level {
        number: SOL_SOCKET,
        name: "SOCKET",
        prefix: "SOL_",
        option_prefix: "SO_",
        kind: "socket",
        options: &[
            ("SNDBUF", SO_SNDBUF),
            ("RCVBUF", SO_RCVBUF),
            ("KEEPALIVE", SO_KEEPALIVE),
        ],
    },
    Level {
        number: IPPROTO_TCP,
        name: "TCP",
        prefix: "IPPROTO_",
        option_prefix: "TCP_",
        kind: "TCP",
        options: &[
            ("NODELAY", TCP_NODELAY),
            ("KEEPIDLE", TCP_KEEPIDLE),
            ("KEEPINTVL", TCP_KEEPINTVL),
            ("KEEPCNT", TCP_KEEPCNT),
        ],
    },
];

/// The options whose `inspect` reads the value as an Integer, as CRuby's
/// `sockopt_inspect` knows each to hold one.
const INTEGER_OPTIONS: [(i32, i32); 4] = [
    (SOL_SOCKET, SO_KEEPALIVE),
    (SOL_SOCKET, SO_RCVBUF),
    (SOL_SOCKET, SO_SNDBUF),
    (IPPROTO_TCP, TCP_NODELAY),
];

/// One socket option: the family, level, and name it is for, and its value
/// as the bytes `setsockopt(2)` takes, kept in the payload so a script's
/// instance variables cannot rewrite it.
#[derive(Clone)]
#[beni::wrap(class = "Socket::Option", name = "Kobako::Socket::Option")]
struct SockOption {
    family: i64,
    level: i32,
    optname: i32,
    data: Vec<u8>,
}

impl SockOption {
    fn of<'a>(mrb: &Mrb, value: Value) -> Result<&'a SockOption, Error> {
        <&SockOption>::try_convert(value, mrb)
    }

    /// The value as a C `int`, as `Socket::Option#int` reads it.
    fn int(&self, mrb: &Mrb) -> Result<i32, Error> {
        let bytes: [u8; 4] = self
            .data
            .as_slice()
            .try_into()
            .map_err(|_| size_error(mrb, self.data.len()))?;
        Ok(i32::from_le_bytes(bytes))
    }
}

/// The `TypeError` CRuby raises for a value that is not one `int`.
fn size_error(mrb: &Mrb, len: usize) -> Error {
    match mrb.exception_type_error() {
        Ok(class) => Error::new(
            mrb,
            class,
            &format!("size differ.  expected as sizeof(int)=4 but {len}"),
        ),
        Err(err) => err,
    }
}

/// Define the level and option constants on `Socket`, `Socket::Option`, and
/// the two `BasicSocket` methods.
pub(super) fn init(mrb: &Mrb, socket: RClass, basic: RClass) -> Result<(), Error> {
    for level in &LEVELS {
        socket.const_set(mrb, format!("{}{}", level.prefix, level.name), level.number)?;
        for (name, number) in level.options {
            socket.const_set(mrb, format!("{}{name}", level.option_prefix), *number)?;
        }
    }
    let option = socket.define_class(mrb, c"Option", mrb.class_object())?;
    SockOption::mark_carriers(mrb)?;
    option.define_singleton_method(mrb, c"new", beni::method!(option_s_new, 4))?;
    option.define_singleton_method(mrb, c"int", beni::method!(option_s_int, 4))?;
    option.define_singleton_method(mrb, c"bool", beni::method!(option_s_bool, 4))?;
    option.define_method(mrb, c"initialize_copy", beni::method!(option_copy, 1))?;
    option.define_method(mrb, c"family", beni::method!(option_family, 0))?;
    option.define_method(mrb, c"level", beni::method!(option_level, 0))?;
    option.define_method(mrb, c"optname", beni::method!(option_optname, 0))?;
    option.define_method(mrb, c"data", beni::method!(option_data, 0))?;
    option.define_method(mrb, c"to_s", beni::method!(option_data, 0))?;
    option.define_method(mrb, c"int", beni::method!(option_int, 0))?;
    option.define_method(mrb, c"bool", beni::method!(option_bool, 0))?;
    option.define_method(mrb, c"inspect", beni::method!(option_inspect, 0))?;
    basic.define_method(mrb, c"setsockopt", beni::method!(socket_setsockopt, -1))?;
    basic.define_method(mrb, c"getsockopt", beni::method!(socket_getsockopt, 2))?;
    Ok(())
}

/// The level `value` names, as CRuby's `rsock_level_arg` reads a Symbol, a
/// String, or an Integer.
fn level_of(mrb: &Mrb, value: Value) -> Result<i32, Error> {
    let Some(name) = name_of(mrb, value)? else {
        return i32::try_convert(value, mrb);
    };
    LEVELS
        .iter()
        .find(|level| name == level.name || name.strip_prefix(level.prefix) == Some(level.name))
        .map(|level| level.number)
        .ok_or_else(|| socket_error(mrb, &format!("unknown protocol level: {name}")))
}

/// The option `value` names at `level`, as CRuby's `rsock_optname_arg` reads
/// a name only at a level it knows.
fn optname_of(mrb: &Mrb, level: i32, value: Value) -> Result<i32, Error> {
    let known = LEVELS.iter().find(|known| known.number == level);
    let (Some(level), Some(name)) = (known, name_of(mrb, value)?) else {
        return i32::try_convert(value, mrb);
    };
    let bare = name.strip_prefix(level.option_prefix).unwrap_or(&name);
    level
        .options
        .iter()
        .find(|(option, _)| *option == bare)
        .map(|(_, number)| *number)
        .ok_or_else(|| {
            socket_error(
                mrb,
                &format!("unknown {} level option name: {name}", level.kind),
            )
        })
}

/// The number of the family `value` names. The guest numbers only IPv4 and
/// IPv6, so another family CRuby knows raises `NotImplementedError`.
fn family_number_of(mrb: &Mrb, value: Value) -> Result<i64, Error> {
    match family_of(mrb, value)? {
        Family::Number(number) => Ok(number),
        Family::Named(name) if name == "INET" => Ok(AF_INET),
        Family::Named(name) if name == "INET6" => Ok(AF_INET6),
        Family::Named(name) => Err(errno::not_implemented(
            mrb,
            &format!(
                "address family {name} is unimplemented: the guest numbers only INET and INET6"
            ),
        )),
    }
}

/// The bytes a value stands for, as CRuby's `setsockopt` reads `true`,
/// `false`, and an Integer as an `int`, and anything else as a String.
fn data_of(mrb: &Mrb, value: Value) -> Result<Vec<u8>, Error> {
    if Qtrue::from_value(value).is_some() || Qfalse::from_value(value).is_some() {
        return Ok(i32::from(value.to_bool()).to_le_bytes().to_vec());
    }
    if beni::Integer::from_value(value).is_some() {
        return Ok(i32::try_convert(value, mrb)?.to_le_bytes().to_vec());
    }
    bytes_of(mrb, value)
}

/// The bytes of the String `value`, as CRuby's `StringValue` takes it.
fn bytes_of(mrb: &Mrb, value: Value) -> Result<Vec<u8>, Error> {
    match RString::from_value(value) {
        // SAFETY: the bytes are copied out before any later mruby call.
        Some(text) => Ok(unsafe { text.as_bytes(mrb) }.to_vec()),
        None => Err(errno::type_error(mrb, value)),
    }
}

/// The `int` a value of `bytes` carries for `setsockopt(2)`, which takes at
/// least that many bytes and reads the first.
fn int_of(mrb: &Mrb, bytes: &[u8]) -> Result<i32, Error> {
    match bytes.get(..4) {
        Some(first) => Ok(i32::from_le_bytes(first.try_into().expect("four bytes"))),
        None => Err(errno::error_by_name(mrb, c"EINVAL", "setsockopt(2)")),
    }
}

/// `setsockopt(level, optname, optval)`, or `setsockopt(option)`, answering
/// 0 as CRuby's does.
fn socket_setsockopt(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<Value, Error> {
    let (level, optname, data) = match args {
        [option] => {
            let option = SockOption::of(mrb, *option)?;
            (option.level, option.optname, option.data.clone())
        }
        [level, optname, value] => {
            let level = level_of(mrb, *level)?;
            let optname = optname_of(mrb, level, *optname)?;
            (level, optname, data_of(mrb, *value)?)
        }
        _ => return Err(Error::argnum(mrb, args.len(), 3, 3)),
    };
    let int = int_of(mrb, &data)?;
    let option = (level, optname);
    let set = match with_tcp_socket(mrb, self_, |socket| Ok(set_tcp_option(socket, option, int)))? {
        Some(set) => set,
        None => with_udp_socket(mrb, self_, |socket| Ok(set_udp_option(socket, option, int)))?
            .unwrap_or(Err(Refusal::Unknown)),
    };
    set.map(|()| 0.into_value(mrb))
        .map_err(|refused| refused.into_error(mrb, level, "setsockopt(2)"))
}

/// Why a socket answered no option: WASI's error, an option WASI has no call
/// for, or a level the socket's protocol does not take.
enum Refusal {
    Wasi(ErrorCode),
    Unknown,
    Unsupported,
}

impl From<ErrorCode> for Refusal {
    fn from(code: ErrorCode) -> Self {
        Self::Wasi(code)
    }
}

impl Refusal {
    /// The `Errno` Linux answers: an unknown option at a level it knows
    /// raises ENOPROTOOPT, and a level it does not ENOTSUP.
    fn into_error(self, mrb: &Mrb, level: i32, call: &str) -> Error {
        let known = LEVELS.iter().any(|known| known.number == level);
        match self {
            Self::Wasi(code) => to_error(mrb, code, "BasicSocket", call),
            Self::Unknown if known => errno::error_by_name(mrb, c"ENOPROTOOPT", call),
            Self::Unknown | Self::Unsupported => errno::error_by_name(mrb, c"ENOTSUP", call),
        }
    }
}

/// A buffer size as Linux reads one, unsigned, and as WASI takes it, never
/// zero: Linux keeps its least for zero, which one byte also asks for.
fn to_buffer_size(int: i32) -> u64 {
    u64::from(int.cast_unsigned()).max(1)
}

/// A hop limit as `do_ip_setsockopt` takes `IP_TTL`: -1 for Linux's default,
/// or 1 to 255.
fn to_hop_limit(int: i32) -> Result<u8, Refusal> {
    match int {
        -1 => Ok(64),
        1..=255 => Ok(u8::try_from(int).expect("within u8")),
        _ => Err(ErrorCode::InvalidArgument.into()),
    }
}

/// A buffer size as Linux reads it back, twice what WASI answers.
fn to_doubled(size: u64) -> i32 {
    i32::try_from(size.saturating_mul(2)).unwrap_or(i32::MAX)
}

/// The socket- and IP-level options every inet socket takes, as Linux's
/// `sk_setsockopt` and `ip_setsockopt` answer them beneath each protocol.
trait InetOptions {
    fn receive_buffer_size(&self) -> Result<u64, ErrorCode>;
    fn set_receive_buffer_size(&self, size: u64) -> Result<(), ErrorCode>;
    fn send_buffer_size(&self) -> Result<u64, ErrorCode>;
    fn set_send_buffer_size(&self, size: u64) -> Result<(), ErrorCode>;
    fn hop_limit(&self) -> Result<u8, ErrorCode>;
    fn set_hop_limit(&self, limit: u8) -> Result<(), ErrorCode>;
}

impl InetOptions for TcpSocket {
    fn receive_buffer_size(&self) -> Result<u64, ErrorCode> {
        TcpSocket::receive_buffer_size(self)
    }
    fn set_receive_buffer_size(&self, size: u64) -> Result<(), ErrorCode> {
        TcpSocket::set_receive_buffer_size(self, size)
    }
    fn send_buffer_size(&self) -> Result<u64, ErrorCode> {
        TcpSocket::send_buffer_size(self)
    }
    fn set_send_buffer_size(&self, size: u64) -> Result<(), ErrorCode> {
        TcpSocket::set_send_buffer_size(self, size)
    }
    fn hop_limit(&self) -> Result<u8, ErrorCode> {
        TcpSocket::hop_limit(self)
    }
    fn set_hop_limit(&self, limit: u8) -> Result<(), ErrorCode> {
        TcpSocket::set_hop_limit(self, limit)
    }
}

impl InetOptions for UdpSocket {
    fn receive_buffer_size(&self) -> Result<u64, ErrorCode> {
        UdpSocket::receive_buffer_size(self)
    }
    fn set_receive_buffer_size(&self, size: u64) -> Result<(), ErrorCode> {
        UdpSocket::set_receive_buffer_size(self, size)
    }
    fn send_buffer_size(&self) -> Result<u64, ErrorCode> {
        UdpSocket::send_buffer_size(self)
    }
    fn set_send_buffer_size(&self, size: u64) -> Result<(), ErrorCode> {
        UdpSocket::set_send_buffer_size(self, size)
    }
    fn hop_limit(&self) -> Result<u8, ErrorCode> {
        self.unicast_hop_limit()
    }
    fn set_hop_limit(&self, limit: u8) -> Result<(), ErrorCode> {
        self.set_unicast_hop_limit(limit)
    }
}

/// Set a socket- or IP-level option of `socket` to `int`; `None` for one
/// its protocol answers itself.
fn set_inet_option(
    socket: &impl InetOptions,
    option: (i32, i32),
    int: i32,
) -> Option<Result<(), Refusal>> {
    let set = match option {
        (SOL_SOCKET, SO_RCVBUF) => socket.set_receive_buffer_size(to_buffer_size(int)),
        (SOL_SOCKET, SO_SNDBUF) => socket.set_send_buffer_size(to_buffer_size(int)),
        (IPPROTO_IP, IP_TTL) => match to_hop_limit(int) {
            Ok(limit) => socket.set_hop_limit(limit),
            Err(refused) => return Some(Err(refused)),
        },
        _ => return None,
    };
    Some(set.map_err(Refusal::from))
}

/// What a socket- or IP-level option of `socket` reads; `None` for one its
/// protocol answers itself.
fn inet_option(socket: &impl InetOptions, option: (i32, i32)) -> Option<Result<i32, Refusal>> {
    let read = match option {
        // WASI reads back the size asked for; Linux reads the doubled one.
        (SOL_SOCKET, SO_RCVBUF) => socket.receive_buffer_size().map(to_doubled),
        (SOL_SOCKET, SO_SNDBUF) => socket.send_buffer_size().map(to_doubled),
        (IPPROTO_IP, IP_TTL) => socket.hop_limit().map(i32::from),
        _ => return None,
    };
    Some(read.map_err(Refusal::from))
}

/// Set the option `(level, optname)` of a TCP socket to `int`, as Linux's
/// `setsockopt(2)` reads it.
fn set_tcp_option(socket: &TcpSocket, option: (i32, i32), int: i32) -> Result<(), Refusal> {
    if let Some(set) = set_inet_option(socket, option, int) {
        return set;
    }
    let set = match option {
        (SOL_SOCKET, SO_KEEPALIVE) => socket.set_keep_alive_enabled(int != 0),
        (IPPROTO_TCP, TCP_KEEPIDLE) => {
            socket.set_keep_alive_idle_time(to_nanos(int, MAX_TCP_KEEPIDLE)?)
        }
        (IPPROTO_TCP, TCP_KEEPINTVL) => {
            socket.set_keep_alive_interval(to_nanos(int, MAX_TCP_KEEPINTVL)?)
        }
        (IPPROTO_TCP, TCP_KEEPCNT) => match u32::try_from(int) {
            Ok(count @ 1..=MAX_TCP_KEEPCNT) => socket.set_keep_alive_count(count),
            _ => Err(ErrorCode::InvalidArgument),
        },
        _ => return Err(Refusal::Unknown),
    };
    Ok(set?)
}

/// `secs` as WASI's nanoseconds, within 1 to `max` as Linux takes a TCP
/// keepalive time, else `invalid-argument`, which reads EINVAL.
fn to_nanos(secs: i32, max: u32) -> Result<u64, Refusal> {
    match u32::try_from(secs) {
        Ok(secs @ 1..) if secs <= max => Ok(u64::from(secs) * NANOS_PER_SEC),
        _ => Err(ErrorCode::InvalidArgument.into()),
    }
}

/// What the option `(level, optname)` of a TCP socket reads, as Linux's
/// `getsockopt(2)` answers it.
fn tcp_option(socket: &TcpSocket, option: (i32, i32)) -> Result<i32, Refusal> {
    if let Some(read) = inet_option(socket, option) {
        return read;
    }
    let secs = |nanos: u64| i32::try_from(nanos / NANOS_PER_SEC).unwrap_or(i32::MAX);
    let read = match option {
        (SOL_SOCKET, SO_KEEPALIVE) => socket.keep_alive_enabled().map(i32::from),
        (IPPROTO_TCP, TCP_KEEPIDLE) => socket.keep_alive_idle_time().map(secs),
        (IPPROTO_TCP, TCP_KEEPINTVL) => socket.keep_alive_interval().map(secs),
        (IPPROTO_TCP, TCP_KEEPCNT) => socket
            .keep_alive_count()
            .map(|count| i32::try_from(count).unwrap_or(i32::MAX)),
        _ => return Err(Refusal::Unknown),
    };
    Ok(read?)
}

/// Set the option `(level, optname)` of a UDP socket to `int`. A TCP option
/// raises ENOPROTOOPT, as `udp_setsockopt` hands it to `ip_setsockopt`.
fn set_udp_option(socket: &UdpSocket, option: (i32, i32), int: i32) -> Result<(), Refusal> {
    set_inet_option(socket, option, int).unwrap_or(Err(Refusal::Unknown))
}

/// What the option `(level, optname)` of a UDP socket reads. A TCP option
/// raises ENOTSUP, as Linux answers `getsockopt` at a level UDP does not take.
fn udp_option(socket: &UdpSocket, option: (i32, i32)) -> Result<i32, Refusal> {
    match (inet_option(socket, option), option) {
        (Some(read), _) => read,
        (None, (IPPROTO_TCP, _)) => Err(Refusal::Unsupported),
        (None, _) => Err(Refusal::Unknown),
    }
}

/// `getsockopt(level, optname)`, answering a `Socket::Option` holding the
/// socket's own setting.
fn socket_getsockopt(
    mrb: &Mrb,
    self_: Value,
    level: Value,
    optname: Value,
) -> Result<Value, Error> {
    let level = level_of(mrb, level)?;
    let optname = optname_of(mrb, level, optname)?;
    let option = (level, optname);
    let read = match with_tcp_socket(mrb, self_, |socket| Ok(tcp_option(socket, option)))? {
        Some(read) => read,
        None => with_udp_socket(mrb, self_, |socket| Ok(udp_option(socket, option)))?
            .unwrap_or(Err(Refusal::Unknown)),
    };
    let value = read.map_err(|refused| refused.into_error(mrb, level, "getsockopt(2)"))?;
    let option = SockOption {
        family: AF_INET,
        level,
        optname,
        data: value.to_le_bytes().to_vec(),
    };
    Ok(mrb.wrap(option).as_value())
}

fn new_option(
    mrb: &Mrb,
    class: RClass,
    (family, level, optname): (Value, Value, Value),
    data: Vec<u8>,
) -> Result<Value, Error> {
    let level_number = level_of(mrb, level)?;
    let option = SockOption {
        family: family_number_of(mrb, family)?,
        level: level_number,
        optname: optname_of(mrb, level_number, optname)?,
        data,
    };
    Ok(mrb.wrap_as(option, class).as_value())
}

fn option_s_new(
    mrb: &Mrb,
    class: RClass,
    family: Value,
    level: Value,
    optname: Value,
    data: Value,
) -> Result<Value, Error> {
    let data = bytes_of(mrb, data)?;
    new_option(mrb, class, (family, level, optname), data)
}

fn option_s_int(
    mrb: &Mrb,
    class: RClass,
    family: Value,
    level: Value,
    optname: Value,
    int: Value,
) -> Result<Value, Error> {
    let data = i32::try_convert(int, mrb)?.to_le_bytes().to_vec();
    new_option(mrb, class, (family, level, optname), data)
}

fn option_s_bool(
    mrb: &Mrb,
    class: RClass,
    family: Value,
    level: Value,
    optname: Value,
    bool: Value,
) -> Result<Value, Error> {
    let data = i32::from(bool.to_bool()).to_le_bytes().to_vec();
    new_option(mrb, class, (family, level, optname), data)
}

/// `dup` and `clone` copy the payload, which nothing changes afterward.
fn option_copy(mrb: &Mrb, self_: RTypedData, orig: Value) -> Result<Value, Error> {
    // A receiver already holding an option keeps it, as an option never changes.
    let _ = self_.init(mrb, SockOption::of(mrb, orig)?.clone());
    Ok(self_.as_value())
}

fn option_family(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    Ok(SockOption::of(mrb, self_)?.family.into_value(mrb))
}

fn option_level(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    Ok(SockOption::of(mrb, self_)?.level.into_value(mrb))
}

fn option_optname(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    Ok(SockOption::of(mrb, self_)?.optname.into_value(mrb))
}

fn option_data(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    Ok(mrb.str_new(&SockOption::of(mrb, self_)?.data).as_value())
}

fn option_int(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    Ok(SockOption::of(mrb, self_)?.int(mrb)?.into_value(mrb))
}

/// The value as a boolean, read from one byte or one `int`, as
/// `Socket::Option#bool` reads either.
fn option_bool(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let option = SockOption::of(mrb, self_)?;
    let on = match option.data.as_slice() {
        [byte] => *byte != 0,
        _ => option.int(mrb)? != 0,
    };
    Ok(on.into_value(mrb))
}

/// `#<Socket::Option: INET SOCKET KEEPALIVE 1>`, naming what CRuby's
/// `sockopt_inspect` names and showing the bytes of a value it does not read.
fn option_inspect(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let option = SockOption::of(mrb, self_)?;
    let family = match option.family {
        AF_INET => "INET".to_string(),
        AF_INET6 => "INET6".to_string(),
        other => format!("family:{other}"),
    };
    let level = LEVELS.iter().find(|level| level.number == option.level);
    let names = match level {
        Some(level) => {
            let optname = level
                .options
                .iter()
                .find(|(_, number)| *number == option.optname)
                .map_or(format!("optname:{}", option.optname), |(name, _)| {
                    (*name).to_string()
                });
            format!("{} {optname}", level.name)
        }
        None => format!("level:{} optname:{}", option.level, option.optname),
    };
    let value = match (
        INTEGER_OPTIONS.contains(&(option.level, option.optname)),
        option.int(mrb),
    ) {
        (true, Ok(int)) => int.to_string(),
        _ => mrb.str_new(&option.data).as_value().inspect(mrb),
    };
    Ok(mrb
        .str_new(format!("#<Socket::Option: {family} {names} {value}>").as_bytes())
        .as_value())
}
