//! The end a host and a port name, as CRuby's `rsock_addrinfo` reads them
//! for `getaddrinfo`: an IPv4 literal, or the address a name leads to.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::time::Duration;

use beni::prelude::*;
use beni::{Error, Integer, IntoValue, Mrb, RClass, TryConvert, Value};

use super::{resolution_error, to_error, EAI_AGAIN, EAI_FAIL, EAI_NONAME, EAI_SERVICE};
use crate::convert::{ensure_fits, ensure_no_nul, string_bytes_of};
use crate::errno;
use crate::io::argument_error;
use crate::wasi::wait_within;
use crate::wasi::wasi::sockets::instance_network::instance_network;
use crate::wasi::wasi::sockets::ip_name_lookup::resolve_addresses;
use crate::wasi::wasi::sockets::network::{
    ErrorCode, IpAddress, IpSocketAddress, Ipv4SocketAddress,
};

/// The words CRuby's `Errno` carries for a call on `host` and `port`, as
/// `rsock_syserr_fail_host_port` inspects the host the script passed.
fn to_peer_message(mrb: &Mrb, call: &str, host: Value, port: impl std::fmt::Display) -> String {
    format!("{call} for {} port {port}", host.inspect(mrb))
}

/// Each IPv4 address `name` leads to, in the order the resolver prefers, as
/// kobako's hands out a virtual one for each; a name with no address raises
/// `Socket::ResolutionError`, as an unallowed one does, a Sandbox with no
/// network fails it for good, as `EAI_FAIL`, and a lookup slower than
/// `timeout` raises `Errno::ETIMEDOUT`, as CRuby's `resolv_timeout` does.
fn resolve(
    mrb: &Mrb,
    name: &str,
    timeout: Option<Duration>,
    call: &str,
) -> Result<Vec<Ipv4Addr>, Error> {
    let to_lookup_error = |code| match code {
        ErrorCode::PermanentResolverFailure => resolution_error(mrb, EAI_FAIL),
        ErrorCode::TemporaryResolverFailure => resolution_error(mrb, EAI_AGAIN),
        ErrorCode::NameUnresolvable | ErrorCode::InvalidArgument => {
            resolution_error(mrb, EAI_NONAME)
        }
        code => to_error(mrb, code, call),
    };
    let stream = resolve_addresses(&instance_network(), name).map_err(to_lookup_error)?;
    let pollable = stream.subscribe();
    let mut found = Vec::new();
    loop {
        match stream.resolve_next_address() {
            Ok(Some(IpAddress::Ipv4((a, b, c, d)))) => found.push(Ipv4Addr::new(a, b, c, d)),
            Ok(Some(IpAddress::Ipv6(_))) => {}
            Ok(None) if found.is_empty() => return Err(resolution_error(mrb, EAI_NONAME)),
            Ok(None) => return Ok(found),
            Err(ErrorCode::WouldBlock) => {
                if !wait_within(&pollable, timeout) {
                    return Err(errno::error_by_name(
                        mrb,
                        c"ETIMEDOUT",
                        "user specified timeout",
                    ));
                }
            }
            Err(code) => return Err(to_lookup_error(code)),
        }
    }
}

/// A lookup that waits as long as it takes.
pub(super) fn no_timeout() -> Result<Option<Duration>, Error> {
    Ok(None)
}

/// The text CRuby's `host_str` hands `getaddrinfo` for `host`: `None` for
/// `nil`; an Integer is the IPv4 address it numbers, as `NUM2UINT` reads it,
/// raising `RangeError` past an `unsigned int`; `""` and `"<any>"` are any
/// address and `"<broadcast>"` the broadcast one; any other value converts
/// as `StringValueCStr` does, so a Symbol raises `TypeError` and a NUL
/// `ArgumentError`, and one of `NI_MAXHOST` bytes raises `ArgumentError`.
pub(super) fn host_text_of(mrb: &Mrb, host: Value) -> Result<Option<String>, Error> {
    if host.is_nil() {
        return Ok(None);
    }
    if Integer::from_value(host).is_some() {
        let number = i64::try_convert(host, mrb)?;
        ensure_fits(mrb, number, INT_TO_UINT, "unsigned int")?;
        return Ok(Some(Ipv4Addr::from(number as u32).to_string()));
    }
    let bytes = cstr_bytes_of(mrb, host)?;
    if bytes.len() >= NI_MAXHOST {
        let message = format!("hostname too long ({})", bytes.len());
        return Err(argument_error(mrb, &message));
    }
    Ok(Some(match &bytes[..] {
        b"" | b"<any>" => Ipv4Addr::UNSPECIFIED.to_string(),
        b"<broadcast>" => Ipv4Addr::BROADCAST.to_string(),
        _ => String::from_utf8_lossy(&bytes).into_owned(),
    }))
}

/// The host `getaddrinfo` looks up for `text`: a host it is not given, or
/// `"*"`, which glibc reads as none, is the loopback, or any address when
/// `passive`.
pub(super) fn node_of(text: Option<String>, passive: bool) -> String {
    match text.filter(|text| text != "*") {
        Some(text) => text,
        None if passive => Ipv4Addr::UNSPECIFIED.to_string(),
        None => Ipv4Addr::LOCALHOST.to_string(),
    }
}

/// What CRuby's `NUM2UINT` takes, wrapping a negative one: an `int`'s least
/// through an `unsigned int`'s most, as `check_uint` bounds it.
const INT_TO_UINT: core::ops::RangeInclusive<i64> = i32::MIN as i64..=u32::MAX as i64;

/// The bytes CRuby's `host_str` holds a host in, its NUL among them.
const NI_MAXHOST: usize = 1025;

/// The bytes of `value` as `StringValueCStr` takes them: a NUL inside raises
/// `ArgumentError`.
fn cstr_bytes_of(mrb: &Mrb, value: Value) -> Result<Vec<u8>, Error> {
    let bytes = string_bytes_of(mrb, value)?;
    ensure_no_nul(mrb, &bytes)?;
    Ok(bytes)
}

/// The address `text` spells, as glibc's `getaddrinfo` reads a numeric host:
/// IPv6 as written, and IPv4 as `__inet_aton_exact` reads it, so `127.1`,
/// `0x7f.1`, and `2130706433` are `127.0.0.1`.
pub(super) fn literal_of(text: &str) -> Option<IpAddr> {
    if text.contains(':') {
        return text.parse::<Ipv6Addr>().ok().map(IpAddr::V6);
    }
    inet_aton_exact(text).map(IpAddr::V4)
}

/// The IPv4 address `text` spells as glibc's `__inet_aton_exact` reads it,
/// the whole text one address: one to four parts, each in C's base 0 (`0x`
/// hex, a leading `0` octal), the last filling the bytes the others leave.
fn inet_aton_exact(text: &str) -> Option<Ipv4Addr> {
    let parts = text.split('.').collect::<Vec<_>>();
    if parts.len() > 4 {
        return None;
    }
    let numbers = parts
        .iter()
        .map(|part| {
            let (digits, radix) = match part.as_bytes() {
                [b'0', b'x' | b'X', ..] => (&part[2..], 16),
                [b'0', _, ..] => (&part[1..], 8),
                _ => (*part, 10),
            };
            let valid = |b: u8| (b as char).is_digit(radix);
            if (digits.is_empty() && radix != 16) || !digits.bytes().all(valid) {
                return None;
            }
            match digits.is_empty() {
                true => Some(0),
                false => u32::from_str_radix(digits, radix).ok(),
            }
        })
        .collect::<Option<Vec<u32>>>()?;
    let (last, leading) = numbers.split_last()?;
    if leading.iter().any(|n| *n > 0xff) {
        return None;
    }
    let room = 32 - 8 * leading.len() as u32;
    if room < 32 && *last >= 1 << room {
        return None;
    }
    let high = leading
        .iter()
        .enumerate()
        .fold(0u32, |ip, (i, n)| ip | n << (24 - 8 * i as u32));
    Some(Ipv4Addr::from(high | last))
}

/// The ends `host` and `port` name for `call`, for a connect or a bind to
/// try in turn: the IPv4 address `host` spells, or each one its name leads
/// to within what `resolv_timeout` reads, with the words CRuby's `Errno`
/// carries for them. Each is read as CRuby's `rsock_addrinfo` reads them
/// with no `AI_PASSIVE`, so `nil` is the loopback. The timeout is read only
/// for a name, as CRuby reads its resolve timeout only where it must resolve
/// one.
pub(super) fn ends_of(
    mrb: &Mrb,
    host: Value,
    port: Value,
    resolv_timeout: &dyn Fn() -> Result<Option<Duration>, Error>,
    call: &str,
) -> Result<(Vec<Ipv4Addr>, u16, String), Error> {
    let host_text = host_text_of(mrb, host)?;
    let service = service_text_of(mrb, port)?;
    ensure_named(mrb, &host_text, &service)?;
    let port_number = port_of_service(mrb, service.as_deref())?;
    let host_text = node_of(host_text, false);
    let call = to_peer_message(mrb, call, host, service.unwrap_or_default());
    let ips = match literal_of(&host_text) {
        Some(IpAddr::V4(ip)) => vec![ip],
        Some(IpAddr::V6(_)) => return Err(errno::error_by_name(mrb, c"EAFNOSUPPORT", &call)),
        None => resolve(mrb, &host_text, resolv_timeout()?, &call)?,
    };
    Ok((ips, port_number, call))
}

/// The local end `host` and `port` name for a bind, and the text naming it in
/// an error, as `ends_of` reads them, a name binding the first address it
/// leads to, as `init_inetsock_internal` takes the first of the peer's
/// family. The host decides which end a bind may take.
pub(super) fn local_of(
    mrb: &Mrb,
    host: Value,
    port: Value,
) -> Result<(Ipv4Addr, u16, String), Error> {
    let (ips, port, call) = ends_of(mrb, host, port, &no_timeout, "bind(2)")?;
    Ok((ips[0], port, call))
}

/// The port `port` names, as `service_text_of` and then `port_of_service`
/// read it.
pub(crate) fn port_of(mrb: &Mrb, port: Value) -> Result<u16, Error> {
    let text = service_text_of(mrb, port)?;
    port_of_service(mrb, text.as_deref())
}

/// The text CRuby's `port_str` hands `getaddrinfo` for `port`: `None` for
/// `nil`, an Integer's decimal digits, and a String as `StringValueCStr`
/// takes it, one of 32 bytes or more raising `ArgumentError`. An Integer
/// past a Fixnum raises `TypeError`, as `port_str` takes it as a String.
pub(super) fn service_text_of(mrb: &Mrb, port: Value) -> Result<Option<String>, Error> {
    if port.is_nil() {
        return Ok(None);
    }
    if Integer::from_value(port).is_some() {
        let number = i64::try_convert(port, mrb)
            .ok()
            .filter(|number| (FIXNUM_MIN..=FIXNUM_MAX).contains(number));
        return Ok(Some(
            number
                .ok_or_else(|| errno::type_error(mrb, port))?
                .to_string(),
        ));
    }
    let bytes = cstr_bytes_of(mrb, port)?;
    if bytes.len() >= SERVICE_TEXT_MAX {
        let message = format!("service name too long ({})", bytes.len());
        return Err(argument_error(mrb, &message));
    }
    Ok(Some(String::from_utf8_lossy(&bytes).into_owned()))
}

/// The port glibc's `getaddrinfo` reads from `service`: none, `""`, or
/// `"*"` is port 0; digits are read with `strtoul`, a sign and leading space
/// allowed, kept in an `int`, a negative one refused, and its low 16 bits
/// taken. The guest has no services database, so any other text raises
/// `EAI_SERVICE`, as Linux without one does.
pub(super) fn port_of_service(mrb: &Mrb, service: Option<&str>) -> Result<u16, Error> {
    let Some(text) = service.filter(|text| !text.is_empty() && *text != "*") else {
        return Ok(0);
    };
    let number = strtoul(text).ok_or_else(|| resolution_error(mrb, EAI_SERVICE))?;
    if (number as u32 as i32) < 0 {
        return Err(resolution_error(mrb, EAI_SERVICE));
    }
    Ok(number as u16)
}

/// Refuse a call naming neither a host nor a service, as glibc's
/// `getaddrinfo` answers `EAI_NONAME` before anything else, `"*"` naming
/// none.
pub(super) fn ensure_named(
    mrb: &Mrb,
    host: &Option<String>,
    service: &Option<String>,
) -> Result<(), Error> {
    let none = |text: &Option<String>| matches!(text.as_deref(), None | Some("*"));
    match none(host) && none(service) {
        true => Err(resolution_error(mrb, EAI_NONAME)),
        false => Ok(()),
    }
}

/// The bytes CRuby's `port_str` holds a service in, its NUL among them.
const SERVICE_TEXT_MAX: usize = 32;

/// What a Fixnum holds on a 64-bit CRuby, past which an Integer is a Bignum.
const FIXNUM_MAX: i64 = (1 << 62) - 1;
const FIXNUM_MIN: i64 = -(1 << 62);

/// The number `text` spells all through, as C's `strtoul` reads it: leading
/// space as `isspace` takes it, then a sign, then digits, a minus negating
/// it and one past `unsigned long` answering the largest; `None` when
/// anything else stands.
pub(super) fn strtoul(text: &str) -> Option<u64> {
    let text = text.trim_start_matches([' ', '\t', '\n', '\u{b}', '\u{c}', '\r']);
    let (negative, digits) = match text.as_bytes().first() {
        Some(b'-') => (true, &text[1..]),
        Some(b'+') => (false, &text[1..]),
        _ => (false, text),
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(match digits.parse::<u64>() {
        Ok(number) if negative => number.wrapping_neg(),
        Ok(number) => number,
        Err(_) => u64::MAX,
    })
}

/// Define `Socket.sockaddr_in`, `Socket.pack_sockaddr_in`, and
/// `Socket.unpack_sockaddr_in`.
pub(super) fn init(mrb: &Mrb, socket: RClass) -> Result<(), Error> {
    for name in [c"sockaddr_in", c"pack_sockaddr_in"] {
        socket.define_singleton_method(mrb, name, beni::method!(socket_s_sockaddr_in, 2))?;
    }
    socket.define_singleton_method(
        mrb,
        c"unpack_sockaddr_in",
        beni::method!(socket_s_unpack_sockaddr_in, 1),
    )?;
    Ok(())
}

/// The addresses `host` names once `node_of` read it: a literal is itself,
/// and a name each address it leads to.
pub(super) fn addresses_of(mrb: &Mrb, host: &str) -> Result<Vec<IpAddr>, Error> {
    Ok(match literal_of(host) {
        Some(ip) => vec![ip],
        None => resolve(mrb, host, None, "getaddrinfo(3)")?
            .into_iter()
            .map(IpAddr::V4)
            .collect(),
    })
}

/// Linux's address families as a `sockaddr` begins with them.
const AF_INET: u16 = super::AF_INET as u16;
const AF_INET6: u16 = super::AF_INET6 as u16;

/// `(ip, port)` as Linux lays out a `sockaddr_in`, or a `sockaddr_in6` for an
/// IPv6 address: the family in host order, the port in network order.
pub(super) fn to_sockaddr(ip: IpAddr, port: u16) -> Vec<u8> {
    let mut sockaddr = Vec::with_capacity(28);
    match ip {
        IpAddr::V4(ip) => {
            sockaddr.extend(AF_INET.to_ne_bytes());
            sockaddr.extend(port.to_be_bytes());
            sockaddr.extend(ip.octets());
            sockaddr.extend([0; 8]);
        }
        IpAddr::V6(ip) => {
            sockaddr.extend(AF_INET6.to_ne_bytes());
            sockaddr.extend(port.to_be_bytes());
            sockaddr.extend([0; 4]);
            sockaddr.extend(ip.octets());
            sockaddr.extend([0; 4]);
        }
    }
    sockaddr
}

/// The end a packed `sockaddr_in` or `sockaddr_in6` holds; a String no IPv4
/// or IPv6 `sockaddr` lays out raises `ArgumentError`, as CRuby's.
pub(super) fn end_of(mrb: &Mrb, sockaddr: Value) -> Result<(IpAddr, u16), Error> {
    let bytes = string_bytes_of(mrb, sockaddr)?;
    if bytes.len() < 2 {
        return Err(argument_error(mrb, "too short sockaddr"));
    }
    let family = bytes
        .get(..2)
        .map(|family| u16::from_ne_bytes([family[0], family[1]]));
    let ip: IpAddr = match (family, bytes.len()) {
        (Some(AF_INET), 16..) => Ipv4Addr::new(bytes[4], bytes[5], bytes[6], bytes[7]).into(),
        (Some(AF_INET6), 28..) => {
            let octets: [u8; 16] = bytes[8..24].try_into().expect("sixteen bytes");
            Ipv6Addr::from(octets).into()
        }
        _ => return Err(argument_error(mrb, "not an AF_INET/AF_INET6 sockaddr")),
    };
    Ok((ip, u16::from_be_bytes([bytes[2], bytes[3]])))
}

/// `Socket.sockaddr_in(port, host)` packs the end `host` and `port` name, a
/// name's first address, as CRuby's packs the first `getaddrinfo` answers.
fn socket_s_sockaddr_in(mrb: &Mrb, _self: Value, port: Value, host: Value) -> Result<Value, Error> {
    let host_text = host_text_of(mrb, host)?;
    let service = service_text_of(mrb, port)?;
    ensure_named(mrb, &host_text, &service)?;
    let port = port_of_service(mrb, service.as_deref())?;
    let ip = addresses_of(mrb, &node_of(host_text, false))?[0];
    Ok(mrb.str_new(&to_sockaddr(ip, port)).as_value())
}

/// `Socket.unpack_sockaddr_in(sockaddr)` answers `[port, ip]`; a String no
/// IPv4 or IPv6 `sockaddr` lays out raises `ArgumentError`, as CRuby's.
fn socket_s_unpack_sockaddr_in(mrb: &Mrb, _self: Value, sockaddr: Value) -> Result<Value, Error> {
    let (ip, port) = end_of(mrb, sockaddr)?;
    let answer = mrb.ary_new();
    answer.push(mrb, i64::from(port).into_value(mrb))?;
    answer.push(mrb, mrb.str_new(ip.to_string().as_bytes()).as_value())?;
    Ok(answer.as_value())
}

/// The `(ip, port)` an IPv4 socket address names, `None` for an IPv6 one.
pub(super) fn ipv4_end_of(address: &IpSocketAddress) -> Option<(Ipv4Addr, u16)> {
    match *address {
        IpSocketAddress::Ipv4(Ipv4SocketAddress {
            port,
            address: (a, b, c, d),
        }) => Some((Ipv4Addr::new(a, b, c, d), port)),
        IpSocketAddress::Ipv6(_) => None,
    }
}

/// `(ip, port)` as WASI names a socket address.
pub(super) fn to_ip_socket_address((ip, port): (Ipv4Addr, u16)) -> IpSocketAddress {
    let [a, b, c, d] = ip.octets();
    IpSocketAddress::Ipv4(Ipv4SocketAddress {
        port,
        address: (a, b, c, d),
    })
}
