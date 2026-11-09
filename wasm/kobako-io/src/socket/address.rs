//! The end a host and a port name, as CRuby's `rsock_addrinfo` reads them
//! for `getaddrinfo`: an IPv4 literal, or the address a name leads to.

use std::net::Ipv4Addr;
use std::time::Duration;

use beni::prelude::*;
use beni::{Error, Integer, Mrb, TryConvert, Value};

use super::{no_network, resolution_error, to_error, EAI_AGAIN, EAI_NONAME, EAI_SERVICE};
use crate::errno;
use crate::wasi::wait_within;
use crate::wasi::wasi::sockets::instance_network::instance_network;
use crate::wasi::wasi::sockets::ip_name_lookup::resolve_addresses;
use crate::wasi::wasi::sockets::network::{
    ErrorCode, IpAddress, IpSocketAddress, Ipv4SocketAddress,
};

/// The words CRuby's `Errno` carries for a call on `host` and `port`.
pub(super) fn for_peer(call: &str, host: &str, port: impl std::fmt::Display) -> String {
    format!("{call} for \"{host}\" port {port}")
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
pub(super) fn peer_of(
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
pub(super) fn to_ip_socket_address((ip, port): (Ipv4Addr, u16)) -> IpSocketAddress {
    let [a, b, c, d] = ip.octets();
    IpSocketAddress::Ipv4(Ipv4SocketAddress {
        port,
        address: (a, b, c, d),
    })
}
