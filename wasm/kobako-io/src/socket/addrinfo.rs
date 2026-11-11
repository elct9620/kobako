//! `Addrinfo`, an address and the kind of socket it is for, as CRuby's
//! `ext/socket/raddrinfo.c` holds one, and the lookups that hand them out.
//! The guest makes no raw socket, so an `Addrinfo` defines no `connect`,
//! `bind`, or `listen`.

use std::net::{IpAddr, Ipv4Addr};

use beni::prelude::*;
use beni::scan_args::get_kwargs;
use beni::value::qnil;
use beni::{
    Error, Integer, IntoValue, Module, Mrb, RArray, RClass, RHash, TryConvert, TypedData, Value,
};

use super::address::{end_of, pack_host, port_of, sockaddr_of};
use super::basic::open_socket_of;
use super::{
    family_of, name_of, resolution_error, socket_error, Family, AF_INET, AF_INET6, EAI_NONAME,
};
use crate::io::argument_error;

/// Linux's socket kinds, and the protocols each IP kind carries.
pub(super) const SOCK_STREAM: i64 = 1;
pub(super) const SOCK_DGRAM: i64 = 2;
const SOCK_RAW: i64 = 3;
const SOCK_RDM: i64 = 4;
const SOCK_SEQPACKET: i64 = 5;
const SOCK_PACKET: i64 = 10;
pub(super) const IPPROTO_TCP: i64 = 6;
pub(super) const IPPROTO_UDP: i64 = 17;

/// The IP protocols CRuby names on Linux, as `inspect` shows a protocol
/// beside a kind it does not imply.
const IP_PROTOCOLS: &[(i64, &str)] = &[
    (1, "IPPROTO_ICMP"),
    (2, "IPPROTO_IGMP"),
    (IPPROTO_TCP, "IPPROTO_TCP"),
    (8, "IPPROTO_EGP"),
    (12, "IPPROTO_PUP"),
    (IPPROTO_UDP, "IPPROTO_UDP"),
    (22, "IPPROTO_IDP"),
    (29, "IPPROTO_TP"),
    (41, "IPPROTO_IPV6"),
    (43, "IPPROTO_ROUTING"),
    (44, "IPPROTO_FRAGMENT"),
    (50, "IPPROTO_ESP"),
    (51, "IPPROTO_AH"),
    (58, "IPPROTO_ICMPV6"),
    (59, "IPPROTO_NONE"),
    (60, "IPPROTO_DSTOPTS"),
    (255, "IPPROTO_RAW"),
];

/// glibc's `getaddrinfo` and `getnameinfo` flags the guest's lookup reads.
const AI_PASSIVE: i64 = 1;
const AI_CANONNAME: i64 = 2;
const AI_NUMERICHOST: i64 = 4;
const NI_NUMERICHOST: i64 = 1;
const NI_NUMERICSERV: i64 = 2;

/// glibc's `EAI_FAMILY` and `EAI_ADDRFAMILY`, with `gai_strerror`'s words for
/// each.
const EAI_FAMILY: (i32, &str) = (-6, "ai_family not supported");
const EAI_ADDRFAMILY: (i32, &str) = (-9, "Address family for hostname not supported");

/// One address and the kind of socket it is for, kept in its payload so a
/// script's instance variables cannot rewrite it.
#[beni::wrap(class = "Addrinfo", name = "Kobako::Addrinfo")]
pub(super) struct Addrinfo {
    ip: IpAddr,
    port: u16,
    pfamily: i64,
    socktype: i64,
    protocol: i64,
    /// The name the address was looked up by, as `inspect` shows it.
    inspectname: Option<String>,
    canonname: Option<String>,
}

impl Addrinfo {
    fn of<'a>(mrb: &Mrb, value: Value) -> Result<&'a Addrinfo, Error> {
        <&Addrinfo>::try_convert(value, mrb)
    }

    fn new(ip: IpAddr, port: u16, (socktype, protocol): (i64, i64)) -> Addrinfo {
        Addrinfo {
            ip,
            port,
            pfamily: afamily_of(ip),
            socktype,
            protocol,
            inspectname: None,
            canonname: None,
        }
    }

    fn wrap(self, mrb: &Mrb) -> Result<Value, Error> {
        let class = mrb.class_get(c"Addrinfo")?;
        Ok(mrb.wrap_as(self, class).as_value())
    }
}

fn afamily_of(ip: IpAddr) -> i64 {
    match ip {
        IpAddr::V4(_) => AF_INET,
        IpAddr::V6(_) => AF_INET6,
    }
}

/// The `Addrinfo` of the end `(ip, port)` a socket of `kind` holds, as
/// `local_address` and `remote_address` answer.
fn from_end(mrb: &Mrb, (ip, port): (Ipv4Addr, u16), kind: (i64, i64)) -> Result<Value, Error> {
    Addrinfo::new(ip.into(), port, kind).wrap(mrb)
}

pub(super) fn init(mrb: &Mrb, socket: RClass, basic: RClass) -> Result<(), Error> {
    for (name, value) in [
        (c"SOCK_STREAM", SOCK_STREAM),
        (c"SOCK_DGRAM", SOCK_DGRAM),
        (c"SOCK_RAW", SOCK_RAW),
        (c"IPPROTO_UDP", IPPROTO_UDP),
        (c"AI_PASSIVE", AI_PASSIVE),
        (c"AI_CANONNAME", AI_CANONNAME),
        (c"AI_NUMERICHOST", AI_NUMERICHOST),
        (c"NI_NUMERICHOST", NI_NUMERICHOST),
        (c"NI_NUMERICSERV", NI_NUMERICSERV),
    ] {
        socket.const_set(mrb, name, value)?;
    }
    socket.define_singleton_method(mrb, c"getaddrinfo", beni::method!(socket_s_getaddrinfo, -1))?;
    basic.define_method(
        mrb,
        c"local_address",
        beni::method!(socket_local_address, 0),
    )?;
    basic.define_method(
        mrb,
        c"remote_address",
        beni::method!(socket_remote_address, 0),
    )?;

    let class = mrb.define_class(c"Addrinfo", mrb.class_object())?;
    Addrinfo::mark_carriers(mrb)?;
    class.define_singleton_method(mrb, c"new", beni::method!(addrinfo_s_new, -1))?;
    class.define_singleton_method(mrb, c"tcp", beni::method!(addrinfo_s_tcp, 2))?;
    class.define_singleton_method(mrb, c"udp", beni::method!(addrinfo_s_udp, 2))?;
    class.define_singleton_method(mrb, c"ip", beni::method!(addrinfo_s_ip, 1))?;
    class.define_singleton_method(
        mrb,
        c"getaddrinfo",
        beni::method!(addrinfo_s_getaddrinfo, -1),
    )?;
    class.define_method(mrb, c"afamily", beni::method!(addrinfo_afamily, 0))?;
    class.define_method(mrb, c"pfamily", beni::method!(addrinfo_pfamily, 0))?;
    class.define_method(mrb, c"socktype", beni::method!(addrinfo_socktype, 0))?;
    class.define_method(mrb, c"protocol", beni::method!(addrinfo_protocol, 0))?;
    class.define_method(mrb, c"canonname", beni::method!(addrinfo_canonname, 0))?;
    class.define_method(mrb, c"ip?", beni::method!(addrinfo_ip_p, 0))?;
    class.define_method(mrb, c"ipv4?", beni::method!(addrinfo_ipv4_p, 0))?;
    class.define_method(mrb, c"ipv6?", beni::method!(addrinfo_ipv6_p, 0))?;
    class.define_method(mrb, c"unix?", beni::method!(addrinfo_unix_p, 0))?;
    class.define_method(mrb, c"ip_address", beni::method!(addrinfo_ip_address, 0))?;
    class.define_method(mrb, c"ip_port", beni::method!(addrinfo_ip_port, 0))?;
    class.define_method(mrb, c"ip_unpack", beni::method!(addrinfo_ip_unpack, 0))?;
    class.define_method(mrb, c"to_sockaddr", beni::method!(addrinfo_to_sockaddr, 0))?;
    class.define_method(mrb, c"to_s", beni::method!(addrinfo_to_sockaddr, 0))?;
    class.define_method(mrb, c"inspect", beni::method!(addrinfo_inspect, 0))?;
    class.define_method(
        mrb,
        c"inspect_sockaddr",
        beni::method!(addrinfo_inspect_sockaddr, 0),
    )?;
    class.define_method(
        mrb,
        c"ipv6_to_ipv4",
        beni::method!(addrinfo_ipv6_to_ipv4, 0),
    )?;
    class.define_method(mrb, c"getnameinfo", beni::method!(addrinfo_getnameinfo, -1))?;
    class.define_method(
        mrb,
        c"family_addrinfo",
        beni::method!(addrinfo_family_addrinfo, -1),
    )?;
    define_predicates(mrb, class)
}

/// The kind `value` names, as CRuby's `rsock_socktype_arg` reads a number or
/// a name with or without `SOCK_`; `nil` names none.
fn socktype_of(mrb: &Mrb, value: Option<Value>) -> Result<i64, Error> {
    let Some(value) = value.filter(|value| !value.is_nil()) else {
        return Ok(0);
    };
    let Some(name) = name_of(mrb, value)? else {
        return i64::try_convert(value, mrb);
    };
    match name.strip_prefix("SOCK_").unwrap_or(&name) {
        "STREAM" => Ok(SOCK_STREAM),
        "DGRAM" => Ok(SOCK_DGRAM),
        "RAW" => Ok(SOCK_RAW),
        _ => Err(socket_error(mrb, &format!("unknown socket type: {name}"))),
    }
}

/// The IP family a lookup is limited to, `None` for either, as CRuby's
/// `rsock_family_arg` reads it; a family that is no IP one raises
/// `EAI_FAMILY`, as glibc's `getaddrinfo` does.
fn lookup_family_of(mrb: &Mrb, value: Option<Value>) -> Result<Option<i64>, Error> {
    let Some(value) = value.filter(|value| !value.is_nil()) else {
        return Ok(None);
    };
    match family_of(mrb, value)? {
        Family::Number(0) => Ok(None),
        Family::Number(number) if number == AF_INET || number == AF_INET6 => Ok(Some(number)),
        Family::Named(name) => match name.as_str() {
            "UNSPEC" => Ok(None),
            "INET" => Ok(Some(AF_INET)),
            "INET6" => Ok(Some(AF_INET6)),
            _ => Err(resolution_error(mrb, EAI_FAMILY)),
        },
        Family::Number(_) => Err(resolution_error(mrb, EAI_FAMILY)),
    }
}

/// What a lookup asks: the family, kind, and protocol to limit it to, and its
/// flags.
struct Hints {
    family: Option<i64>,
    socktype: i64,
    protocol: i64,
    flags: i64,
}

/// The `Addrinfo`s `node` and `service` lead to, as glibc's `getaddrinfo`
/// answers: one per kind the hints leave open, for the address a literal
/// spells or a name leads to, the canonical name on the first when asked.
/// A name leads to its virtual address, and `nil` to the loopback, or to any
/// address with `AI_PASSIVE`.
fn lookup(mrb: &Mrb, node: Value, service: Value, hints: &Hints) -> Result<Vec<Addrinfo>, Error> {
    let (port, service_text) = port_of(mrb, service)?;
    let name = match node.is_nil() || Integer::from_value(node).is_some() {
        true => None,
        false => Some(node.to_r_string(mrb)?.to_string(mrb)?),
    };
    let numeric = name
        .as_deref()
        .is_none_or(|name| name.parse::<IpAddr>().is_ok());
    if hints.flags & AI_NUMERICHOST != 0 && !numeric {
        return Err(resolution_error(mrb, EAI_NONAME));
    }
    let ip = match node.is_nil() && hints.flags & AI_PASSIVE != 0 {
        true => Ipv4Addr::UNSPECIFIED.into(),
        false => pack_host(mrb, node)?,
    };
    if hints.family.is_some_and(|family| family != afamily_of(ip)) {
        return Err(resolution_error(mrb, EAI_ADDRFAMILY));
    }
    // What `inspect` shows past the address and port, as CRuby's
    // `make_inspectname` drops what they already show.
    let mut inspectname = name.clone().filter(|name| *name != ip.to_string());
    if !service.is_nil() && service_text != port.to_string() {
        inspectname = Some(format!(
            "{}:{service_text}",
            inspectname.unwrap_or_default()
        ));
    }
    // glibc's table: a raw socket takes whichever protocol is asked.
    let kinds = [
        (SOCK_STREAM, Some(IPPROTO_TCP)),
        (SOCK_DGRAM, Some(IPPROTO_UDP)),
        (SOCK_RAW, None),
    ];
    let mut found = Vec::new();
    for (socktype, protocol) in kinds {
        if hints.socktype != 0 && hints.socktype != socktype {
            continue;
        }
        if hints.protocol != 0 && protocol.is_some_and(|protocol| protocol != hints.protocol) {
            continue;
        }
        let protocol = protocol.unwrap_or(hints.protocol);
        let mut info = Addrinfo::new(ip, port, (socktype, protocol));
        info.inspectname = inspectname.clone();
        if found.is_empty() && hints.flags & AI_CANONNAME != 0 && !numeric {
            info.canonname = name.clone();
        }
        found.push(info);
    }
    Ok(found)
}

/// The hints `getaddrinfo`'s arguments after `node` and `service` give.
fn hints_of(mrb: &Mrb, args: &[Value]) -> Result<Hints, Error> {
    let at = |i: usize| args.get(i).copied();
    let flags = match at(3).filter(|flags| !flags.is_nil()) {
        Some(flags) => i64::try_convert(flags, mrb)?,
        None => 0,
    };
    let protocol = match at(2).filter(|protocol| !protocol.is_nil()) {
        Some(protocol) => i64::try_convert(protocol, mrb)?,
        None => 0,
    };
    Ok(Hints {
        family: lookup_family_of(mrb, at(0))?,
        socktype: socktype_of(mrb, at(1))?,
        protocol,
        flags,
    })
}

/// `Addrinfo.getaddrinfo(nodename, service, family = nil, socktype = nil,
/// protocol = nil, flags = nil, timeout: nil)` answers every `Addrinfo` the
/// name and service lead to. `timeout:` is taken and read no further, as
/// CRuby's `call_getaddrinfo` on Linux never reads it.
fn addrinfo_s_getaddrinfo(mrb: &Mrb, _self: Value, args: &[Value]) -> Result<Value, Error> {
    let mut args = args.to_vec();
    if let Some(options) = args.last().copied().and_then(RHash::from_value) {
        args.pop();
        get_kwargs::<_, (), (Option<Value>,), ()>(mrb, options, &[], &["timeout"])?;
    }
    let (node, service, rest) = match args.as_slice() {
        [node, service, rest @ ..] if rest.len() <= 4 => (*node, *service, rest),
        _ => return Err(Error::argnum(mrb, args.len(), 2, 6)),
    };
    let found = lookup(mrb, node, service, &hints_of(mrb, rest)?)?;
    let answer = mrb.ary_new();
    for info in found {
        answer.push(mrb, info.wrap(mrb)?)?;
    }
    Ok(answer.as_value())
}

/// The first `Addrinfo` `host` and `port` lead to for a socket of `socktype`
/// and `protocol`, as `Addrinfo.tcp` and `Addrinfo.udp` answer.
fn first_of(
    mrb: &Mrb,
    host: Value,
    port: Value,
    (socktype, protocol): (i64, i64),
) -> Result<Value, Error> {
    let hints = Hints {
        family: None,
        socktype,
        protocol,
        flags: 0,
    };
    let mut found = lookup(mrb, host, port, &hints)?;
    found.remove(0).wrap(mrb)
}

/// `Addrinfo.tcp(host, port)` answers the TCP `Addrinfo` they lead to.
fn addrinfo_s_tcp(mrb: &Mrb, _self: Value, host: Value, port: Value) -> Result<Value, Error> {
    first_of(mrb, host, port, (SOCK_STREAM, IPPROTO_TCP))
}

/// `Addrinfo.udp(host, port)` answers the UDP `Addrinfo` they lead to.
fn addrinfo_s_udp(mrb: &Mrb, _self: Value, host: Value, port: Value) -> Result<Value, Error> {
    first_of(mrb, host, port, (SOCK_DGRAM, IPPROTO_UDP))
}

/// `Addrinfo.ip(host)` answers the address `host` leads to, for no kind of
/// socket.
fn addrinfo_s_ip(mrb: &Mrb, _self: Value, host: Value) -> Result<Value, Error> {
    let hints = Hints {
        family: None,
        socktype: 0,
        protocol: 0,
        flags: 0,
    };
    let mut info = lookup(mrb, host, qnil().as_value(), &hints)?.remove(0);
    (info.socktype, info.protocol) = (0, 0);
    info.wrap(mrb)
}

/// `Addrinfo.new(sockaddr, family = nil, socktype = nil, protocol = nil)`
/// wraps a packed address, or CRuby's `["AF_INET", port, host, ip]`; a
/// packed address of no IP family raises `ArgumentError`, since the guest
/// holds no other.
fn addrinfo_s_new(mrb: &Mrb, _self: Value, args: &[Value]) -> Result<Value, Error> {
    let (sockaddr, rest) = match args {
        [sockaddr, rest @ ..] if rest.len() <= 3 => (*sockaddr, rest),
        _ => return Err(Error::argnum(mrb, args.len(), 1, 4)),
    };
    let (ip, port, inspectname) = match RArray::from_value(sockaddr) {
        Some(parts) => {
            let port: u16 = parts
                .entry::<Value>(mrb, 1)
                .and_then(|port| u16::try_convert(port, mrb))?;
            let host: Value = parts.entry(mrb, 2)?;
            let ip: Value = parts.entry(mrb, 3)?;
            let ip = ip
                .to_r_string(mrb)?
                .to_string(mrb)?
                .parse::<IpAddr>()
                .map_err(|_| argument_error(mrb, "not an AF_INET/AF_INET6 sockaddr"))?;
            let host = (!host.is_nil())
                .then(|| host.to_r_string(mrb)?.to_string(mrb))
                .transpose()?;
            (ip, port, host.filter(|host| *host != ip.to_string()))
        }
        None => {
            let (ip, port) = end_of(mrb, sockaddr)?;
            (ip, port, None)
        }
    };
    let at = |i: usize| rest.get(i).copied();
    let protocol = match at(2).filter(|protocol| !protocol.is_nil()) {
        Some(protocol) => i64::try_convert(protocol, mrb)?,
        None => 0,
    };
    let mut info = Addrinfo::new(ip, port, (socktype_of(mrb, at(1))?, protocol));
    // A packed address names no protocol family, as CRuby's leaves it 0; an
    // array names one by its address family.
    info.pfamily = match lookup_family_of(mrb, at(0))? {
        Some(family) => family,
        None if RArray::from_value(sockaddr).is_some() => afamily_of(ip),
        None => 0,
    };
    info.inspectname = inspectname;
    info.wrap(mrb)
}

fn addrinfo_afamily(mrb: &Mrb, self_: Value) -> Result<i64, Error> {
    Ok(afamily_of(Addrinfo::of(mrb, self_)?.ip))
}

fn addrinfo_pfamily(mrb: &Mrb, self_: Value) -> Result<i64, Error> {
    Ok(Addrinfo::of(mrb, self_)?.pfamily)
}

fn addrinfo_socktype(mrb: &Mrb, self_: Value) -> Result<i64, Error> {
    Ok(Addrinfo::of(mrb, self_)?.socktype)
}

fn addrinfo_protocol(mrb: &Mrb, self_: Value) -> Result<i64, Error> {
    Ok(Addrinfo::of(mrb, self_)?.protocol)
}

fn addrinfo_canonname(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    Ok(match &Addrinfo::of(mrb, self_)?.canonname {
        Some(name) => mrb.str_new(name.as_bytes()).as_value(),
        None => qnil().as_value(),
    })
}

/// Every `Addrinfo` the guest holds is an IP one.
fn addrinfo_ip_p(mrb: &Mrb, self_: Value) -> Result<bool, Error> {
    Addrinfo::of(mrb, self_).map(|_| true)
}

fn addrinfo_ipv4_p(mrb: &Mrb, self_: Value) -> Result<bool, Error> {
    Ok(Addrinfo::of(mrb, self_)?.ip.is_ipv4())
}

fn addrinfo_ipv6_p(mrb: &Mrb, self_: Value) -> Result<bool, Error> {
    Ok(Addrinfo::of(mrb, self_)?.ip.is_ipv6())
}

fn addrinfo_unix_p(mrb: &Mrb, self_: Value) -> Result<bool, Error> {
    Addrinfo::of(mrb, self_).map(|_| false)
}

fn addrinfo_ip_address(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let ip = Addrinfo::of(mrb, self_)?.ip.to_string();
    Ok(mrb.str_new(ip.as_bytes()).as_value())
}

fn addrinfo_ip_port(mrb: &Mrb, self_: Value) -> Result<i64, Error> {
    Ok(i64::from(Addrinfo::of(mrb, self_)?.port))
}

/// `ip_unpack` answers `[ip_address, ip_port]`.
fn addrinfo_ip_unpack(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let info = Addrinfo::of(mrb, self_)?;
    let answer = mrb.ary_new();
    answer.push(mrb, mrb.str_new(info.ip.to_string().as_bytes()).as_value())?;
    answer.push(mrb, i64::from(info.port).into_value(mrb))?;
    Ok(answer.as_value())
}

fn addrinfo_to_sockaddr(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let info = Addrinfo::of(mrb, self_)?;
    Ok(mrb.str_new(&sockaddr_of(info.ip, info.port)).as_value())
}

/// The address as CRuby's `inspect_sockaddr` shows it: the port after it
/// when there is one, an IPv6 address in brackets then.
fn sockaddr_text(info: &Addrinfo) -> String {
    match (info.ip, info.port) {
        (ip, 0) => ip.to_string(),
        (IpAddr::V4(ip), port) => format!("{ip}:{port}"),
        (IpAddr::V6(ip), port) => format!("[{ip}]:{port}"),
    }
}

fn addrinfo_inspect_sockaddr(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let text = sockaddr_text(Addrinfo::of(mrb, self_)?);
    Ok(mrb.str_new(text.as_bytes()).as_value())
}

/// `inspect` as CRuby's `addrinfo_inspect` builds it: the address, a family
/// that differs from it, the kind, the canonical name, and the name looked
/// up.
fn addrinfo_inspect(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let info = Addrinfo::of(mrb, self_)?;
    let mut text = format!("#<Addrinfo: {}", sockaddr_text(info));
    if info.pfamily != 0 && info.pfamily != afamily_of(info.ip) {
        text += &match info.pfamily {
            AF_INET => " PF_INET".to_string(),
            AF_INET6 => " PF_INET6".to_string(),
            family => format!(" PF_???({family})"),
        };
    }
    let internet = info.pfamily == AF_INET || info.pfamily == AF_INET6;
    match (info.socktype, info.protocol) {
        (SOCK_STREAM, 0 | IPPROTO_TCP) if internet => text += " TCP",
        (SOCK_DGRAM, 0 | IPPROTO_UDP) if internet => text += " UDP",
        (socktype, protocol) => {
            text += &match socktype {
                0 => String::new(),
                SOCK_STREAM => " SOCK_STREAM".to_string(),
                SOCK_DGRAM => " SOCK_DGRAM".to_string(),
                SOCK_RAW => " SOCK_RAW".to_string(),
                SOCK_RDM => " SOCK_RDM".to_string(),
                SOCK_SEQPACKET => " SOCK_SEQPACKET".to_string(),
                SOCK_PACKET => " SOCK_PACKET".to_string(),
                socktype => format!(" SOCK_???({socktype})"),
            };
            text += &match protocol {
                0 => String::new(),
                protocol => match IP_PROTOCOLS.iter().find(|(number, _)| *number == protocol) {
                    Some((_, name)) if internet => format!(" {name}"),
                    _ => format!(" UNKNOWN_PROTOCOL({protocol})"),
                },
            };
        }
    }
    if let Some(canonname) = &info.canonname {
        text += &format!(" {canonname}");
    }
    if let Some(name) = &info.inspectname {
        text += &format!(" ({name})");
    }
    text.push('>');
    Ok(mrb.str_new(text.as_bytes()).as_value())
}

/// `ipv6_to_ipv4` answers the IPv4 `Addrinfo` an IPv4-mapped or
/// IPv4-compatible address carries, `nil` for any other.
fn addrinfo_ipv6_to_ipv4(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let info = Addrinfo::of(mrb, self_)?;
    let IpAddr::V6(ip) = info.ip else {
        return Ok(qnil().as_value());
    };
    let carried = ip
        .to_ipv4_mapped()
        .or_else(|| is_v4_compat(&ip).then(|| ip.to_ipv4()).flatten());
    // CRuby's carries the kind and the names over, and no port.
    let Some(v4) = carried else {
        return Ok(qnil().as_value());
    };
    let mut converted = Addrinfo::new(v4.into(), 0, (info.socktype, info.protocol));
    converted.inspectname = info.inspectname.clone();
    converted.canonname = info.canonname.clone();
    converted.wrap(mrb)
}

/// `getnameinfo(flags = 0)` answers `[host, service]`. The guest names no
/// host and keeps no services database, so both answer as numbers, as
/// `NI_NUMERICHOST` and `NI_NUMERICSERV` ask.
fn addrinfo_getnameinfo(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<Value, Error> {
    if let Some(flags) = args.first().filter(|_| args.len() == 1) {
        i64::try_convert(*flags, mrb)?;
    } else if !args.is_empty() {
        return Err(Error::argnum(mrb, args.len(), 0, 1));
    }
    let info = Addrinfo::of(mrb, self_)?;
    let answer = mrb.ary_new();
    answer.push(mrb, mrb.str_new(info.ip.to_string().as_bytes()).as_value())?;
    answer.push(
        mrb,
        mrb.str_new(info.port.to_string().as_bytes()).as_value(),
    )?;
    Ok(answer.as_value())
}

/// `family_addrinfo(host, port)` answers the `Addrinfo` they lead to of this
/// one's family and kind.
fn addrinfo_family_addrinfo(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<Value, Error> {
    let (host, port) = match args {
        [] => return Err(argument_error(mrb, "no address specified")),
        [host, port] => (*host, *port),
        _ => {
            let message = format!(
                "IP address needs host and port but {} arguments given",
                args.len()
            );
            return Err(argument_error(mrb, &message));
        }
    };
    let info = Addrinfo::of(mrb, self_)?;
    let hints = Hints {
        family: Some(afamily_of(info.ip)),
        socktype: info.socktype,
        protocol: info.protocol,
        flags: 0,
    };
    lookup(mrb, host, port, &hints)?.remove(0).wrap(mrb)
}

/// `Socket.getaddrinfo(host, service, family = nil, socktype = nil, protocol
/// = nil, flags = nil, reverse_lookup = nil)` answers each address as
/// `["AF_INET", port, host, ip, family, socktype, protocol]`. The guest names
/// no host, so a reverse lookup answers the address.
fn socket_s_getaddrinfo(mrb: &Mrb, _self: Value, args: &[Value]) -> Result<Value, Error> {
    let (node, service, rest) = match args {
        [node, service, rest @ ..] if rest.len() <= 5 => (*node, *service, rest),
        _ => return Err(Error::argnum(mrb, args.len(), 2, 7)),
    };
    let hints = hints_of(mrb, &rest[..rest.len().min(4)])?;
    let answer = mrb.ary_new();
    for info in lookup(mrb, node, service, &hints)? {
        let family = afamily_of(info.ip);
        let ip = mrb.str_new(info.ip.to_string().as_bytes()).as_value();
        let entry = mrb.ary_new();
        let name: &[u8] = if family == AF_INET {
            b"AF_INET"
        } else {
            b"AF_INET6"
        };
        entry.push(mrb, mrb.str_new(name).as_value())?;
        entry.push(mrb, i64::from(info.port).into_value(mrb))?;
        entry.push(mrb, ip)?;
        entry.push(mrb, ip)?;
        for number in [family, info.socktype, info.protocol] {
            entry.push(mrb, number.into_value(mrb))?;
        }
        answer.push(mrb, entry.as_value())?;
    }
    Ok(answer.as_value())
}

/// `local_address` answers the socket's own end as an `Addrinfo` of its kind.
fn socket_local_address(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let socket = open_socket_of(mrb, self_)?;
    from_end(mrb, socket.local_end(mrb)?, socket.kind())
}

/// `remote_address` answers the peer as an `Addrinfo` of the socket's kind.
fn socket_remote_address(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let socket = open_socket_of(mrb, self_)?;
    from_end(mrb, socket.peer_end(mrb)?, socket.kind())
}

/// Whether an IPv6 address is IPv4-compatible: the IPv4 address in its last
/// four bytes, zeros before, past `::` and `::1`, as `IN6_IS_ADDR_V4COMPAT`.
fn is_v4_compat(ip: &std::net::Ipv6Addr) -> bool {
    let octets = ip.octets();
    octets[..12].iter().all(|byte| *byte == 0)
        && u32::from_be_bytes(octets[12..].try_into().unwrap_or_default()) > 1
}

/// Whether an IPv6 address is multicast of `scope`, as `IN6_IS_ADDR_MC_*`.
fn is_multicast_of_scope(ip: &IpAddr, scope: u8) -> bool {
    matches!(ip, IpAddr::V6(ip) if ip.octets()[0] == 0xff && ip.octets()[1] & 0x0f == scope)
}

macro_rules! predicates {
    ($($name:literal => $func:ident => $test:expr),* $(,)?) => {
        $(
            fn $func(mrb: &Mrb, self_: Value) -> Result<bool, Error> {
                let test: fn(&IpAddr) -> bool = $test;
                Ok(test(&Addrinfo::of(mrb, self_)?.ip))
            }
        )*

        /// Define the address predicates CRuby's `Addrinfo` answers, each
        /// false for an address of the other family.
        fn define_predicates(mrb: &Mrb, class: RClass) -> Result<(), Error> {
            $( class.define_method(mrb, $name, beni::method!($func, 0))?; )*
            Ok(())
        }
    };
}

predicates! {
    c"ipv4_loopback?" => ipv4_loopback_p => |ip| matches!(ip, IpAddr::V4(ip) if ip.is_loopback()),
    c"ipv4_private?" => ipv4_private_p => |ip| matches!(ip, IpAddr::V4(ip) if ip.is_private()),
    c"ipv4_multicast?" => ipv4_multicast_p => |ip| matches!(ip, IpAddr::V4(ip) if ip.is_multicast()),
    c"ipv6_loopback?" => ipv6_loopback_p => |ip| matches!(ip, IpAddr::V6(ip) if ip.is_loopback()),
    c"ipv6_unspecified?" => ipv6_unspecified_p => |ip| matches!(ip, IpAddr::V6(ip) if ip.is_unspecified()),
    c"ipv6_linklocal?" => ipv6_linklocal_p =>
        |ip| matches!(ip, IpAddr::V6(ip) if ip.segments()[0] & 0xffc0 == 0xfe80),
    c"ipv6_sitelocal?" => ipv6_sitelocal_p =>
        |ip| matches!(ip, IpAddr::V6(ip) if ip.segments()[0] & 0xffc0 == 0xfec0),
    c"ipv6_unique_local?" => ipv6_unique_local_p =>
        |ip| matches!(ip, IpAddr::V6(ip) if ip.segments()[0] & 0xfe00 == 0xfc00),
    c"ipv6_multicast?" => ipv6_multicast_p => |ip| matches!(ip, IpAddr::V6(ip) if ip.is_multicast()),
    c"ipv6_v4mapped?" => ipv6_v4mapped_p => |ip| matches!(ip, IpAddr::V6(ip) if ip.to_ipv4_mapped().is_some()),
    c"ipv6_v4compat?" => ipv6_v4compat_p => |ip| matches!(ip, IpAddr::V6(ip) if is_v4_compat(ip)),
    c"ipv6_mc_nodelocal?" => ipv6_mc_nodelocal_p => |ip| is_multicast_of_scope(ip, 0x1),
    c"ipv6_mc_linklocal?" => ipv6_mc_linklocal_p => |ip| is_multicast_of_scope(ip, 0x2),
    c"ipv6_mc_sitelocal?" => ipv6_mc_sitelocal_p => |ip| is_multicast_of_scope(ip, 0x5),
    c"ipv6_mc_orglocal?" => ipv6_mc_orglocal_p => |ip| is_multicast_of_scope(ip, 0x8),
    c"ipv6_mc_global?" => ipv6_mc_global_p => |ip| is_multicast_of_scope(ip, 0xe),
}
