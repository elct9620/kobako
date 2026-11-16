//! `Addrinfo`, an address and the kind of socket it is for, as CRuby's
//! `ext/socket/raddrinfo.c` holds one, and the lookups that hand them out.
//! The guest makes no raw socket, so an `Addrinfo` defines no `connect`,
//! `bind`, or `listen`.

use std::net::{IpAddr, Ipv4Addr};

use beni::prelude::*;
use beni::scan_args::get_kwargs;
use beni::value::qnil;
use beni::{Error, Integer, IntoValue, Module, Mrb, RArray, RClass, TryConvert, TypedData, Value};

use super::address::{
    addresses_of, end_of, given, literal_of, named_of, node_of, port_of_service, strtoul,
    to_sockaddr,
};
use super::basic::open_socket_of;
use super::ip::ensure_reverse_lookup_flag;
use super::{
    bare_family, family_of, name_of, resolution_error, socket_error, to_resolution_error,
    Constants, Family, AF_INET, AF_INET6, EAI_ADDRFAMILY, EAI_BADFLAGS, EAI_FAMILY, EAI_NONAME,
    EAI_SOCKTYPE,
};
use crate::convert::string_value_of;
use crate::io::{argument_error, split_options};

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
const AI_NUMERICSERV: i64 = 0x400;
/// Every flag glibc's `getaddrinfo` takes: `AI_PASSIVE` through `AI_ALL`,
/// `AI_ADDRCONFIG`, `AI_NUMERICSERV`, and the IDN ones.
const AI_FLAGS: i64 = 0x7ff;
const NI_NUMERICHOST: i64 = 1;
const NI_NUMERICSERV: i64 = 2;
/// Every flag glibc's `getnameinfo` takes, its `NI_IDN` ones among them.
const NI_FLAGS: i64 = 0xff;

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
    pub(super) fn of<'a>(mrb: &Mrb, value: Value) -> Result<&'a Addrinfo, Error> {
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

pub(super) fn init(mrb: &Mrb, constants: &Constants, basic: RClass) -> Result<(), Error> {
    constants.set_all(
        mrb,
        &[
            ("SOCK_STREAM", SOCK_STREAM),
            ("SOCK_DGRAM", SOCK_DGRAM),
            ("SOCK_RAW", SOCK_RAW),
            ("IPPROTO_UDP", IPPROTO_UDP),
            ("AI_PASSIVE", AI_PASSIVE),
            ("AI_CANONNAME", AI_CANONNAME),
            ("AI_NUMERICHOST", AI_NUMERICHOST),
            ("AI_NUMERICSERV", AI_NUMERICSERV),
            ("NI_NUMERICHOST", NI_NUMERICHOST),
            ("NI_NUMERICSERV", NI_NUMERICSERV),
            ("NI_NOFQDN", 4),
            ("NI_NAMEREQD", 8),
            ("NI_DGRAM", 16),
        ],
    )?;
    let socket = mrb.class_get(c"Socket")?;
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
        return crate::convert::num2int(mrb, value).map(i64::from);
    };
    match name.strip_prefix("SOCK_").unwrap_or(&name) {
        "STREAM" => Ok(SOCK_STREAM),
        "DGRAM" => Ok(SOCK_DGRAM),
        "RAW" => Ok(SOCK_RAW),
        "RDM" => Ok(SOCK_RDM),
        "SEQPACKET" => Ok(SOCK_SEQPACKET),
        "PACKET" => Ok(SOCK_PACKET),
        _ => Err(socket_error(mrb, &format!("unknown socket type: {name}"))),
    }
}

/// The family a lookup is limited to, `None` for either, as CRuby's
/// `rsock_family_arg` reads it: an IP family by its number, and any other
/// as `OTHER_FAMILY`, which glibc refuses in its own turn.
fn lookup_family_of(mrb: &Mrb, value: Option<Value>) -> Result<Option<i64>, Error> {
    let Some(value) = value.filter(|value| !value.is_nil()) else {
        return Ok(None);
    };
    Ok(match family_of(mrb, value)? {
        Family::Number(0) => None,
        Family::Named(name) if name == "UNSPEC" => None,
        family => Some(family.ip_number().unwrap_or(OTHER_FAMILY)),
    })
}

/// A family no IP lookup takes, standing for every one CRuby names but glibc
/// refuses.
const OTHER_FAMILY: i64 = -1;

/// `family` once glibc's `getaddrinfo` judged it: one other than IPv4's and
/// IPv6's raises `EAI_FAMILY`.
fn ip_family(mrb: &Mrb, family: Option<i64>) -> Result<Option<i64>, Error> {
    match family {
        Some(family) if ![AF_INET, AF_INET6].contains(&family) => {
            Err(resolution_error(mrb, EAI_FAMILY))
        }
        family => Ok(family),
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
/// spells or each a name leads to, the canonical name on the first when
/// asked. A name leads to its virtual addresses, and `nil` to the loopback,
/// or to any address with `AI_PASSIVE`.
fn lookup(mrb: &Mrb, node: Value, service: Value, hints: &Hints) -> Result<Vec<Addrinfo>, Error> {
    // CRuby's `host_str` and `port_str` read both before glibc is asked,
    // which then refuses in `getaddrinfo`'s order: no name, a flag, the
    // family, a service `AI_NUMERICSERV` refuses, a kind, then the service
    // itself. `"*"` names none.
    let (host_text, service_text) = named_of(mrb, node, service)?;
    let unnamed = given(host_text.as_deref()).is_none();
    let numeric_service = given(service_text.as_deref())
        .is_none_or(|text| text.is_empty() || strtoul(text).is_some());
    // CRuby answers a literal address at a numeric port itself, as its
    // `numeric_getaddrinfo` does, reading no flag glibc would refuse.
    let numeric =
        !unnamed && numeric_service && host_text.as_deref().and_then(literal_of).is_some();
    if hints.flags & !AI_FLAGS != 0 && !numeric {
        return Err(resolution_error(mrb, EAI_BADFLAGS));
    }
    if hints.flags & AI_CANONNAME != 0 && unnamed {
        return Err(resolution_error(mrb, EAI_BADFLAGS));
    }
    ip_family(mrb, hints.family)?;
    if hints.flags & AI_NUMERICSERV != 0 && !numeric_service {
        return Err(resolution_error(mrb, EAI_NONAME));
    }
    let kinds = kinds_of(mrb, hints)?;
    let port = port_of_service(mrb, service_text.as_deref())?;
    let service_text = service_text.unwrap_or_default();
    let passive = hints.flags & AI_PASSIVE != 0;
    // The node as written, which `inspect` shows, as CRuby's
    // `make_inspectname` keeps what the script spelled.
    let name = match node.is_nil() || Integer::from_value(node).is_some() {
        true => None,
        false => Some(string_value_of(mrb, node)?.to_string(mrb)?),
    };
    // The node as `host_str` and glibc read it, so `""`, `<any>`, and
    // `<broadcast>` are the literals CRuby makes of them.
    let host = node_of(host_text.as_deref(), passive);
    let numeric = literal_of(&host).is_some();
    if hints.flags & AI_NUMERICHOST != 0 && !numeric {
        return Err(resolution_error(mrb, EAI_NONAME));
    }
    let ips = addresses_of(mrb, &host)?;
    if hints
        .family
        .is_some_and(|family| ips.iter().all(|ip| family != afamily_of(*ip)))
    {
        return Err(resolution_error(mrb, EAI_ADDRFAMILY));
    }
    // Each address in the resolver's order, then each kind, as glibc's
    // `gaih_inet` lists them; only the first carries the canonical name.
    let mut found = Vec::new();
    for ip in ips {
        if hints.family.is_some_and(|family| family != afamily_of(ip)) {
            continue;
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
        for &(socktype, protocol) in &kinds {
            let mut info = Addrinfo::new(ip, port, (socktype, protocol));
            info.inspectname = inspectname.clone();
            if found.is_empty() && hints.flags & AI_CANONNAME != 0 && !numeric {
                info.canonname = name.clone();
            }
            found.push(info);
        }
    }
    Ok(found)
}

/// The address, port, and `inspect` name CRuby's `["AF_INET", port, host,
/// ip]` holds: the family a String naming an IP one, the port an Integer
/// `NUM2INT` takes and glibc reads as a service, the host `nil` or a String,
/// and the ip a numeric address of `family`, or of the array's own family
/// when `family` names none, as CRuby asks glibc for one; the kinds
/// `socktype` and `protocol` name must be ones glibc's table holds.
fn parts_of(
    mrb: &Mrb,
    parts: RArray,
    family: Option<i64>,
    socktype: i64,
    protocol: i64,
) -> Result<(IpAddr, u16, Option<String>), Error> {
    // CRuby reads each part before glibc's numeric lookup judges the kind,
    // the service, and the address in turn.
    let named = string_value_of(mrb, parts.entry(mrb, 0)?)?.to_string(mrb)?;
    let named = Family::Named(bare_family(&named).to_string())
        .ip_number()
        .ok_or_else(|| socket_error(mrb, &format!("unknown address family: {named}")))?;
    let number = i64::from(crate::convert::num2int(mrb, parts.entry(mrb, 1)?)?);
    let host: Value = parts.entry(mrb, 2)?;
    let host = (!host.is_nil())
        .then(|| string_value_of(mrb, host)?.to_string(mrb))
        .transpose()?;
    let address = string_value_of(mrb, parts.entry(mrb, 3)?)?.to_string(mrb)?;
    let hints = Hints {
        family: None,
        socktype,
        protocol,
        flags: 0,
    };
    let family = ip_family(mrb, family)?.unwrap_or(named);
    kinds_of(mrb, &hints)?;
    let port = port_of_service(mrb, Some(&number.to_string()))?;
    let ip = literal_of(&address).ok_or_else(|| resolution_error(mrb, EAI_NONAME))?;
    if family != afamily_of(ip) {
        return Err(resolution_error(mrb, EAI_ADDRFAMILY));
    }
    let mut inspectname = host.filter(|host| *host != ip.to_string());
    if number != i64::from(port) {
        inspectname = Some(format!("{}:{number}", inspectname.unwrap_or_default()));
    }
    Ok((ip, port, inspectname))
}

/// The `Addrinfo`s a lookup answers, as `lookup` does, once an Integer
/// `service` fits an `int`: CRuby's `make_inspectname` reads it so for each
/// entry it made, raising `RangeError` past one after the lookup.
fn addrinfos_of(
    mrb: &Mrb,
    node: Value,
    service: Value,
    hints: &Hints,
) -> Result<Vec<Addrinfo>, Error> {
    let found = lookup(mrb, node, service, hints)?;
    if Integer::from_value(service).is_some() {
        crate::convert::num2int(mrb, service)?;
    }
    Ok(found)
}

/// The kinds and protocols `hints` leave open, from glibc's table, where a
/// raw socket takes whichever protocol is asked. None left raises
/// `EAI_SOCKTYPE` before the service or the node is looked up, as
/// `gaih_inet` checks its table first. WASI has no SCTP, UDP-Lite, or DCCP
/// socket, so the kinds glibc answers with one are refused too.
fn kinds_of(mrb: &Mrb, hints: &Hints) -> Result<Vec<(i64, i64)>, Error> {
    let table = [
        (SOCK_STREAM, Some(IPPROTO_TCP)),
        (SOCK_DGRAM, Some(IPPROTO_UDP)),
        (SOCK_RAW, None),
    ];
    let kinds: Vec<_> = table
        .into_iter()
        .filter(|(socktype, _)| hints.socktype == 0 || hints.socktype == *socktype)
        .filter(|(_, protocol)| {
            hints.protocol == 0 || protocol.is_none_or(|protocol| protocol == hints.protocol)
        })
        .map(|(socktype, protocol)| (socktype, protocol.unwrap_or(hints.protocol)))
        .collect();
    if kinds.is_empty() {
        return Err(resolution_error(mrb, EAI_SOCKTYPE));
    }
    Ok(kinds)
}

/// The hints `getaddrinfo`'s arguments after `node` and `service` give, read
/// in CRuby's `call_getaddrinfo` order: the family, the kind, the protocol,
/// then the flags.
fn hints_of(mrb: &Mrb, args: &[Value]) -> Result<Hints, Error> {
    let at = |i: usize| args.get(i).copied();
    Ok(Hints {
        family: lookup_family_of(mrb, at(0))?,
        socktype: socktype_of(mrb, at(1))?,
        protocol: int_or_zero_of(mrb, at(2))?,
        flags: int_or_zero_of(mrb, at(3))?,
    })
}

/// A protocol or flags argument as CRuby's `NUM2INT` reads it, `nil` or none
/// being 0.
fn int_or_zero_of(mrb: &Mrb, value: Option<Value>) -> Result<i64, Error> {
    match value.filter(|value| !value.is_nil()) {
        Some(value) => crate::convert::num2int(mrb, value).map(i64::from),
        None => Ok(0),
    }
}

/// `Addrinfo.getaddrinfo(nodename, service, family = nil, socktype = nil,
/// protocol = nil, flags = nil, timeout: nil)` answers every `Addrinfo` the
/// name and service lead to. `timeout:` is taken and read no further, as
/// CRuby's `call_getaddrinfo` on Linux never reads it.
fn addrinfo_s_getaddrinfo(mrb: &Mrb, _self: Value, args: &[Value]) -> Result<Value, Error> {
    let (args, options) = split_options(args);
    if let Some(options) = options {
        get_kwargs::<_, (), (Option<Value>,), ()>(mrb, options, &[], &["timeout"])?;
    }
    let (node, service, rest) = match args {
        [node, service, rest @ ..] if rest.len() <= 4 => (*node, *service, rest),
        _ => return Err(Error::argnum(mrb, args.len(), 2, 6)),
    };
    let found = addrinfos_of(mrb, node, service, &hints_of(mrb, rest)?)?;
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
    addrinfos_of(mrb, host, port, &hints)?.remove(0).wrap(mrb)
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
/// holds no other. An array is read as CRuby's `init_addrinfo_getaddrinfo`
/// reads it, a numeric lookup of its address limited to its family and to
/// `family`, `socktype`, and `protocol`.
fn addrinfo_s_new(mrb: &Mrb, _self: Value, args: &[Value]) -> Result<Value, Error> {
    let (sockaddr, rest) = match args {
        [sockaddr, rest @ ..] if rest.len() <= 3 => (*sockaddr, rest),
        _ => return Err(Error::argnum(mrb, args.len(), 1, 4)),
    };
    let at = |i: usize| rest.get(i).copied();
    // CRuby's `addrinfo_initialize` reads the family, the kind, and the
    // protocol before the address.
    let family = lookup_family_of(mrb, at(0))?;
    let socktype = socktype_of(mrb, at(1))?;
    let protocol = int_or_zero_of(mrb, at(2))?;
    let (ip, port, inspectname) = match RArray::from_value(sockaddr) {
        Some(parts) => parts_of(mrb, parts, family, socktype, protocol)?,
        None => {
            let (ip, port) = end_of(mrb, sockaddr)?;
            (ip, port, None)
        }
    };
    let mut info = Addrinfo::new(ip, port, (socktype, protocol));
    // A packed address names no protocol family, as CRuby's leaves it 0; an
    // array names one by its address family.
    info.pfamily = match ip_family(mrb, family)? {
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
    Ok(mrb.str_new(&to_sockaddr(info.ip, info.port)).as_value())
}

/// The address as CRuby's `inspect_sockaddr` shows it: the port after it
/// when there is one, an IPv6 address in brackets then.
fn sockaddr_text_of(info: &Addrinfo) -> String {
    match (info.ip, info.port) {
        (ip, 0) => ip.to_string(),
        (IpAddr::V4(ip), port) => format!("{ip}:{port}"),
        (IpAddr::V6(ip), port) => format!("[{ip}]:{port}"),
    }
}

fn addrinfo_inspect_sockaddr(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let text = sockaddr_text_of(Addrinfo::of(mrb, self_)?);
    Ok(mrb.str_new(text.as_bytes()).as_value())
}

/// `inspect` as CRuby's `addrinfo_inspect` builds it: the address, a family
/// that differs from it, the kind, the canonical name, and the name looked
/// up.
fn addrinfo_inspect(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let info = Addrinfo::of(mrb, self_)?;
    let mut text = format!("#<Addrinfo: {}", sockaddr_text_of(info));
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
        let flags = match flags.is_nil() {
            true => 0,
            false => i64::from(crate::convert::num2int(mrb, *flags)?),
        };
        if flags & !NI_FLAGS != 0 {
            let (code, words) = EAI_BADFLAGS;
            return Err(to_resolution_error(
                mrb,
                code,
                &format!("getnameinfo: {words}"),
            ));
        }
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
    addrinfos_of(mrb, host, port, &hints)?.remove(0).wrap(mrb)
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
    if let Some(flag) = rest.get(4) {
        ensure_reverse_lookup_flag(mrb, *flag)?;
    }
    let answer = mrb.ary_new();
    for info in lookup(mrb, node, service, &hints)? {
        let family = afamily_of(info.ip);
        let entry = super::ip::to_addr_array(mrb, (info.ip, info.port))?;
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
