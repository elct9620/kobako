//! `BasicSocket#setsockopt`, `#getsockopt`, and `Socket::Option`, as CRuby's
//! `ext/socket` reads a level, a name, and a value. An option reaches the host
//! only as the WASI call that stands for it; one WASI has no call for answers
//! `Errno::ENOPROTOOPT`, as Linux answers an option it does not know.

use beni::prelude::*;
use beni::{
    Error, ExceptionClass, IntoValue, Mrb, Qfalse, Qtrue, RClass, RString, RTypedData, Symbol,
    TryConvert, TypedData, Value,
};

use crate::errno;
use crate::socket::{to_error, with_tcp_socket, AF_INET, AF_INET6};

const SOL_SOCKET: i32 = 1;
const SO_KEEPALIVE: i32 = 9;

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
        number: 0,
        name: "IP",
        prefix: "IPPROTO_",
        option_prefix: "IP_",
        kind: "IP",
        options: &[("TTL", 2)],
    },
    Level {
        number: SOL_SOCKET,
        name: "SOCKET",
        prefix: "SOL_",
        option_prefix: "SO_",
        kind: "socket",
        options: &[("SNDBUF", 7), ("RCVBUF", 8), ("KEEPALIVE", SO_KEEPALIVE)],
    },
    Level {
        number: 6,
        name: "TCP",
        prefix: "IPPROTO_",
        option_prefix: "TCP_",
        kind: "TCP",
        options: &[
            ("NODELAY", 1),
            ("KEEPIDLE", 4),
            ("KEEPINTVL", 5),
            ("KEEPCNT", 6),
        ],
    },
];

/// The options whose `inspect` reads the value as an Integer, as CRuby's
/// `sockopt_inspect` knows each to hold one.
const INTEGER_OPTIONS: [(i32, i32); 4] = [(1, 9), (1, 8), (1, 7), (6, 1)];

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

/// Define the level and option constants on `Socket`, `Socket::Option`, and
/// the two `BasicSocket` methods.
pub(crate) fn init(mrb: &Mrb, socket: RClass, basic: RClass) -> Result<(), Error> {
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

/// The name a Symbol or a String spells, `None` for any other value.
fn spelled(mrb: &Mrb, value: Value) -> Result<Option<String>, Error> {
    if let Some(symbol) = Symbol::from_value(value) {
        return symbol.name(mrb).map(Some);
    }
    match RString::from_value(value) {
        Some(text) => text.to_string(mrb).map(Some),
        None => Ok(None),
    }
}

/// The level `value` names, as CRuby's `rsock_level_arg` reads a Symbol, a
/// String, or an Integer.
fn level_of(mrb: &Mrb, value: Value) -> Result<i32, Error> {
    let Some(name) = spelled(mrb, value)? else {
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
    let (Some(level), Some(name)) = (known, spelled(mrb, value)?) else {
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

/// The family `value` names: IPv4 or IPv6, as the guest's sockets know no
/// other.
fn family_of(mrb: &Mrb, value: Value) -> Result<i64, Error> {
    let Some(name) = spelled(mrb, value)? else {
        return i64::try_convert(value, mrb);
    };
    let bare = name
        .strip_prefix("AF_")
        .or_else(|| name.strip_prefix("PF_"))
        .unwrap_or(&name);
    match bare {
        "INET" => Ok(AF_INET),
        "INET6" => Ok(AF_INET6),
        _ => Err(socket_error(mrb, &format!("unknown socket domain: {name}"))),
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
    let set = with_tcp_socket(mrb, self_, |socket| match (level, optname) {
        (SOL_SOCKET, SO_KEEPALIVE) => {
            let on = int_of(mrb, &data)? != 0;
            socket
                .set_keep_alive_enabled(on)
                .map_err(|code| to_error(mrb, code, "TCPSocket", "setsockopt(2)"))
        }
        _ => Err(unknown_option(mrb, level, "setsockopt(2)")),
    })?;
    match set {
        Some(()) => Ok(0.into_value(mrb)),
        None => Err(unknown_option(mrb, level, "setsockopt(2)")),
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
    let value = with_tcp_socket(mrb, self_, |socket| match (level, optname) {
        (SOL_SOCKET, SO_KEEPALIVE) => socket
            .keep_alive_enabled()
            .map(i32::from)
            .map_err(|code| to_error(mrb, code, "TCPSocket", "getsockopt(2)")),
        _ => Err(unknown_option(mrb, level, "getsockopt(2)")),
    })?;
    let Some(value) = value else {
        return Err(unknown_option(mrb, level, "getsockopt(2)"));
    };
    let option = SockOption {
        family: AF_INET,
        level,
        optname,
        data: value.to_le_bytes().to_vec(),
    };
    Ok(mrb.wrap(option).as_value())
}

/// The `Errno` Linux answers for an option the socket has no answer to: an
/// unknown name at a level it knows, or a level it does not.
fn unknown_option(mrb: &Mrb, level: i32, call: &str) -> Error {
    let name = match LEVELS.iter().any(|known| known.number == level) {
        true => c"ENOPROTOOPT",
        false => c"ENOTSUP",
    };
    errno::error_by_name(mrb, name, call)
}

fn new_option(
    mrb: &Mrb,
    class: RClass,
    (family, level, optname): (Value, Value, Value),
    data: Vec<u8>,
) -> Result<Value, Error> {
    let level_number = level_of(mrb, level)?;
    let option = SockOption {
        family: family_of(mrb, family)?,
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
