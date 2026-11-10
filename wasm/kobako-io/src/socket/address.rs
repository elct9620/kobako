//! The end a host and a port name, as CRuby's `rsock_addrinfo` reads them
//! for `getaddrinfo`: an IPv4 literal, or the address a name leads to.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::time::Duration;

use beni::prelude::*;
use beni::{Error, Integer, IntoValue, Mrb, RClass, TryConvert, Value};

use super::{no_network, resolution_error, to_error, EAI_AGAIN, EAI_NONAME, EAI_SERVICE};
use crate::errno;
use crate::io::argument_error;
use crate::wasi::wait_within;
use crate::wasi::wasi::sockets::instance_network::instance_network;
use crate::wasi::wasi::sockets::ip_name_lookup::resolve_addresses;
use crate::wasi::wasi::sockets::network::{
    ErrorCode, IpAddress, IpSocketAddress, Ipv4SocketAddress,
};

/// The words CRuby's `Errno` carries for a call on `host` and `port`.
fn for_peer(call: &str, host: &str, port: impl std::fmt::Display) -> String {
    format!("{call} for \"{host}\" port {port}")
}

/// The IPv4 address `name` leads to, as kobako's resolver hands out a virtual
/// one; a name with no address raises `Socket::ResolutionError`, as an
/// unallowed one does, and a lookup slower than `timeout` raises
/// `Errno::ETIMEDOUT`, as CRuby's `resolv_timeout` does.
fn resolve(
    mrb: &Mrb,
    name: &str,
    timeout: Option<Duration>,
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
                if !wait_within(&pollable, timeout) {
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
/// `host` spells, or the one its name leads to within `resolv_timeout`.
pub(super) fn peer_of(
    mrb: &Mrb,
    host: Value,
    port: Value,
    resolv_timeout: Option<Duration>,
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
        Err(_) => resolve(mrb, &host_text, resolv_timeout, class, &call)?,
    };
    Ok((ip, port, call))
}

/// The local end `host` and `port` name for a bind, and the text naming it in
/// an error, as CRuby's `rsock_addrinfo` reads them with `AI_PASSIVE`: `nil`,
/// `""`, and `"<any>"` stand for any address and `"<broadcast>"` for the
/// broadcast one; any other host resolves as a peer's does. The host decides
/// which end a bind may take.
pub(super) fn local_of(
    mrb: &Mrb,
    host: Value,
    port: Value,
    class: &str,
) -> Result<(Ipv4Addr, u16, String), Error> {
    let text = match host.is_nil() {
        true => String::new(),
        false => host.to_r_string(mrb)?.to_string(mrb)?,
    };
    let ip = match text.as_str() {
        "" | "<any>" => Ipv4Addr::UNSPECIFIED,
        "<broadcast>" => Ipv4Addr::BROADCAST,
        _ => return peer_of(mrb, host, port, None, class, "bind(2)"),
    };
    let (port, port_text) = port_of(mrb, port)?;
    Ok((ip, port, for_peer("bind(2)", &text, port_text)))
}

/// The port `port` names, and its text, as Linux's `getaddrinfo` reads what
/// CRuby hands it: `nil` or `""` is port 0, and an Integer or a String of
/// digits keeps its low 16 bits. The guest has no services database, so any
/// other String raises `EAI_SERVICE`, as Linux without one does.
pub(super) fn port_of(mrb: &Mrb, port: Value) -> Result<(u16, String), Error> {
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

/// The address `host` names to pack, as CRuby's `rsock_addrinfo` reads it:
/// `nil` is the loopback, an Integer the IPv4 address it numbers, `""` and
/// `"<any>"` any address, and `"<broadcast>"` the broadcast one. A literal is
/// itself, and a name the address it leads to within `timeout`. The guest has
/// no IPv6, so `nil` is IPv4's loopback.
pub(super) fn pack_host(
    mrb: &Mrb,
    host: Value,
    timeout: Option<Duration>,
) -> Result<IpAddr, Error> {
    if host.is_nil() {
        return Ok(Ipv4Addr::LOCALHOST.into());
    }
    if Integer::from_value(host).is_some() {
        return Ok(Ipv4Addr::from(i64::try_convert(host, mrb)? as u32).into());
    }
    let text = host.to_r_string(mrb)?.to_string(mrb)?;
    Ok(match text.as_str() {
        "" | "<any>" => Ipv4Addr::UNSPECIFIED.into(),
        "<broadcast>" => Ipv4Addr::BROADCAST.into(),
        _ => match text.parse::<IpAddr>() {
            Ok(ip) => ip,
            Err(_) => resolve(mrb, &text, timeout, "Socket", "getaddrinfo(3)")?.into(),
        },
    })
}

/// Linux's address families as a `sockaddr` begins with them.
const AF_INET: u16 = 2;
const AF_INET6: u16 = 10;

/// `(ip, port)` as Linux lays out a `sockaddr_in`, or a `sockaddr_in6` for an
/// IPv6 address: the family in host order, the port in network order.
pub(super) fn sockaddr_of(ip: IpAddr, port: u16) -> Vec<u8> {
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
    let bytes = Vec::<u8>::from_value(sockaddr).ok_or_else(|| errno::type_error(mrb, sockaddr))?;
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

/// `Socket.sockaddr_in(port, host)` packs the end `host` and `port` name.
fn socket_s_sockaddr_in(mrb: &Mrb, _self: Value, port: Value, host: Value) -> Result<Value, Error> {
    let (port, _) = port_of(mrb, port)?;
    let ip = pack_host(mrb, host, None)?;
    Ok(mrb.str_new(&sockaddr_of(ip, port)).as_value())
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

/// `(ip, port)` as WASI names a socket address.
pub(super) fn to_ip_socket_address((ip, port): (Ipv4Addr, u16)) -> IpSocketAddress {
    let [a, b, c, d] = ip.octets();
    IpSocketAddress::Ipv4(Ipv4SocketAddress {
        port,
        address: (a, b, c, d),
    })
}
