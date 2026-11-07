//! What the guest may reach, compiled for one Invocation from every entry's
//! endpoints, as an nftables ruleset with a default policy of drop. The fixed
//! sets, `metadata` and `reserved`, are kobako's own and sit in no entry, so no
//! setting reaches them.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use super::{Endpoint, EntryError, Protocol};
use crate::vfs::FilesystemError;

/// Where an address falls, by IANA's special-purpose registries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddressSet {
    /// Globally reachable unicast.
    Public,
    /// RFC 1918, shared address space, and unique local addresses.
    Private,
    /// Loopback and this host's network.
    Local,
    /// Link-local addresses and the cloud metadata endpoints, never reached.
    Metadata,
    /// Multicast, broadcast, documentation, transition, and the virtual
    /// range, never reached.
    Reserved,
}

/// The cloud metadata endpoints beyond link-local, never reached.
const METADATA_V4: [Ipv4Addr; 2] = [
    Ipv4Addr::new(100, 100, 100, 200),
    Ipv4Addr::new(168, 63, 129, 16),
];

const METADATA_V6: [Ipv6Addr; 3] = [
    Ipv6Addr::new(0xfd00, 0xec2, 0, 0, 0, 0, 0, 0x254),
    Ipv6Addr::new(0xfd00, 0xec2, 0, 0, 0, 0, 0, 0x23),
    Ipv6Addr::new(0xfd20, 0xce, 0, 0, 0, 0, 0, 0x254),
];

fn within(ip: Ipv4Addr, net: [u8; 4], prefix: u32) -> bool {
    let mask = u32::MAX.checked_shl(32 - prefix).unwrap_or(0);
    u32::from(ip) & mask == u32::from(Ipv4Addr::from(net)) & mask
}

fn within_v6(ip: Ipv6Addr, net: Ipv6Addr, prefix: u32) -> bool {
    let mask = u128::MAX.checked_shl(128 - prefix).unwrap_or(0);
    u128::from(ip) & mask == u128::from(net) & mask
}

/// The IPv4 address an IPv6 one carries, as a mapped, compatible, or NAT64
/// address does, so each is judged as the IPv4 inside.
fn unwrap_v4(ip: Ipv6Addr) -> Option<Ipv4Addr> {
    if let Some(v4) = ip.to_ipv4_mapped() {
        return Some(v4);
    }
    let nat64 = Ipv6Addr::new(0x64, 0xff9b, 0, 0, 0, 0, 0, 0);
    let compatible = Ipv6Addr::UNSPECIFIED;
    let carried = Ipv4Addr::from((u128::from(ip) & 0xffff_ffff) as u32);
    match within_v6(ip, nat64, 96) || (within_v6(ip, compatible, 96) && u32::from(carried) > 1) {
        true => Some(carried),
        false => None,
    }
}

/// The set `ip` falls in, judging an IPv6 address carrying an IPv4 one as
/// that IPv4 address.
pub fn address_set(ip: IpAddr) -> AddressSet {
    match ip {
        IpAddr::V4(ip) => v4_set(ip),
        IpAddr::V6(ip) => match unwrap_v4(ip) {
            Some(v4) => v4_set(v4),
            None => v6_set(ip),
        },
    }
}

fn v4_set(ip: Ipv4Addr) -> AddressSet {
    let globally_reachable = [
        ([192, 0, 0, 9], 32),
        ([192, 0, 0, 10], 32),
        ([192, 31, 196, 0], 24),
        ([192, 52, 193, 0], 24),
        ([192, 175, 48, 0], 24),
    ];
    if METADATA_V4.contains(&ip) || within(ip, [169, 254, 0, 0], 16) {
        AddressSet::Metadata
    } else if within(ip, [127, 0, 0, 0], 8) || within(ip, [0, 0, 0, 0], 8) {
        AddressSet::Local
    } else if within(ip, [10, 0, 0, 0], 8)
        || within(ip, [172, 16, 0, 0], 12)
        || within(ip, [192, 168, 0, 0], 16)
        || within(ip, [100, 64, 0, 0], 10)
    {
        AddressSet::Private
    } else if globally_reachable
        .iter()
        .any(|(net, prefix)| within(ip, *net, *prefix))
    {
        AddressSet::Public
    } else if within(ip, [192, 0, 0, 0], 24)
        || within(ip, [192, 0, 2, 0], 24)
        || within(ip, [192, 88, 99, 0], 24)
        || within(ip, [198, 18, 0, 0], 15)
        || within(ip, [198, 51, 100, 0], 24)
        || within(ip, [203, 0, 113, 0], 24)
        || within(ip, [224, 0, 0, 0], 4)
        || within(ip, [240, 0, 0, 0], 4)
    {
        AddressSet::Reserved
    } else {
        AddressSet::Public
    }
}

fn v6_set(ip: Ipv6Addr) -> AddressSet {
    let net = |a: u16, b: u16| Ipv6Addr::new(a, b, 0, 0, 0, 0, 0, 0);
    if METADATA_V6.contains(&ip) || within_v6(ip, net(0xfe80, 0), 10) {
        AddressSet::Metadata
    } else if ip == Ipv6Addr::LOCALHOST || ip == Ipv6Addr::UNSPECIFIED {
        AddressSet::Local
    } else if within_v6(ip, net(0xfc00, 0), 7) {
        AddressSet::Private
    } else if within_v6(ip, net(0xff00, 0), 8)
        || within_v6(ip, net(0x2001, 0xdb8), 32)
        || within_v6(ip, net(0x2002, 0), 16)
        || within_v6(ip, net(0x2001, 0), 32)
        || within_v6(ip, net(0x0100, 0), 64)
    {
        AddressSet::Reserved
    } else {
        AddressSet::Public
    }
}

/// What an endpoint's host names.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Host {
    /// An exact name.
    Name(String),
    /// Every name ending in this suffix on a label boundary, as `*.` gives it.
    Wildcard(String),
    /// An address or a prefix, reaching what it names.
    Prefix(IpAddr, u8),
}

fn parse_host(host: &str) -> Result<Host, EntryError> {
    if let Ok(ip) = host.parse::<IpAddr>() {
        let prefix = if ip.is_ipv4() { 32 } else { 128 };
        return Ok(Host::Prefix(ip, prefix));
    }
    if let Some((ip, prefix)) = host.split_once('/') {
        let ip = ip
            .parse::<IpAddr>()
            .map_err(|_| EntryError(format!("an endpoint's prefix is no address: {host}")))?;
        let longest = if ip.is_ipv4() { 32 } else { 128 };
        let prefix = prefix
            .parse::<u8>()
            .ok()
            .filter(|p| *p <= longest)
            .ok_or_else(|| EntryError(format!("an endpoint's prefix is out of range: {host}")))?;
        return Ok(Host::Prefix(ip, prefix));
    }
    let name = host.trim_end_matches('.').to_ascii_lowercase();
    match name.strip_prefix("*.") {
        Some(suffix) if !suffix.is_empty() => Ok(Host::Wildcard(suffix.to_string())),
        _ => Ok(Host::Name(name)),
    }
}

fn contains(net: IpAddr, prefix: u8, ip: IpAddr) -> bool {
    match (net, ip) {
        (IpAddr::V4(net), IpAddr::V4(ip)) => within(ip, net.octets(), u32::from(prefix)),
        (IpAddr::V6(net), IpAddr::V6(ip)) => within_v6(ip, net, u32::from(prefix)),
        _ => false,
    }
}

struct Rule {
    host: Host,
    port: u16,
    protocol: Protocol,
}

/// Every endpoint one Invocation's entries let the guest reach.
pub struct Ruleset {
    rules: Vec<Rule>,
}

impl Ruleset {
    /// The ruleset of `endpoints`, refused for an endpoint whose host names no
    /// address, prefix, or name.
    pub fn new(endpoints: impl IntoIterator<Item = Endpoint>) -> Result<Self, EntryError> {
        let rules = endpoints
            .into_iter()
            .map(|endpoint| {
                Ok(Rule {
                    host: parse_host(&endpoint.host)?,
                    port: endpoint.port,
                    protocol: endpoint.protocol,
                })
            })
            .collect::<Result<_, EntryError>>()?;
        Ok(Self { rules })
    }

    /// Whether the guest may open a `protocol` connection to `addr`, the
    /// literal address it named, as nftables judges each address: refused
    /// unless an endpoint with an address host holds it, and refused in the
    /// fixed sets whatever an endpoint says. An `http` endpoint never admits
    /// raw TCP, so its rules cannot be framed around.
    pub fn admit_literal(
        &self,
        addr: SocketAddr,
        protocol: Protocol,
    ) -> Result<(), FilesystemError> {
        if matches!(
            address_set(addr.ip()),
            AddressSet::Metadata | AddressSet::Reserved
        ) {
            return Err(FilesystemError::PermissionDenied);
        }
        let ip = match addr.ip() {
            IpAddr::V6(v6) => unwrap_v4(v6).map_or(addr.ip(), IpAddr::V4),
            v4 => v4,
        };
        let matched = self.rules.iter().any(|rule| {
            rule.port == addr.port()
                && rule.protocol == protocol
                && matches!(rule.host, Host::Prefix(net, prefix) if contains(net, prefix, ip))
        });
        match matched {
            true => Ok(()),
            false => Err(FilesystemError::PermissionDenied),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // @behavior NW-012
    #[test]
    fn each_address_falls_in_its_set() {
        let set = |ip: &str| address_set(ip.parse().unwrap());
        assert_eq!(
            [
                set("93.184.216.34"),
                set("10.1.2.3"),
                set("100.64.0.1"),
                set("127.0.0.1"),
                set("::1"),
                set("169.254.169.254"),
                set("fd00:ec2::254"),
                set("100.100.100.200"),
                set("224.0.0.1"),
                set("198.18.0.1"),
                set("255.255.255.255"),
                set("::ffff:169.254.169.254"),
                set("64:ff9b::a9fe:a9fe"),
            ],
            [
                AddressSet::Public,
                AddressSet::Private,
                AddressSet::Private,
                AddressSet::Local,
                AddressSet::Local,
                AddressSet::Metadata,
                AddressSet::Metadata,
                AddressSet::Metadata,
                AddressSet::Reserved,
                AddressSet::Reserved,
                AddressSet::Reserved,
                AddressSet::Metadata,
                AddressSet::Metadata,
            ],
            "address_set must class each address by IANA's registries, judging an IPv6 address \
             carrying an IPv4 one as the IPv4 inside"
        );
    }

    fn ruleset(endpoints: &[(&str, u16, Protocol)]) -> Ruleset {
        Ruleset::new(endpoints.iter().map(|(host, port, protocol)| {
            Endpoint::new(*host, *port)
                .unwrap()
                .with_protocol(*protocol)
        }))
        .expect("each host parses")
    }

    // @behavior NW-013
    #[test]
    fn a_literal_passes_only_an_endpoint_naming_its_address() {
        let rules = ruleset(&[
            ("127.0.0.1", 4567, Protocol::Tcp),
            ("10.0.0.0/8", 6379, Protocol::Tcp),
        ]);
        let admit =
            |addr: &str, protocol| rules.admit_literal(addr.parse().unwrap(), protocol).is_ok();
        assert_eq!(
            [
                admit("127.0.0.1:4567", Protocol::Tcp),
                admit("10.9.9.9:6379", Protocol::Tcp),
                admit("127.0.0.1:4568", Protocol::Tcp),
                admit("127.0.0.2:4567", Protocol::Tcp),
                admit("127.0.0.1:4567", Protocol::Udp),
            ],
            [true, true, false, false, false],
            "Ruleset::admit_literal must pass an address only through an endpoint whose address \
             host holds it, at its port and protocol"
        );
    }

    // @behavior NW-014
    #[test]
    fn the_fixed_sets_refuse_whatever_an_endpoint_names() {
        let rules = ruleset(&[
            ("169.254.169.254", 80, Protocol::Tcp),
            ("224.0.0.1", 80, Protocol::Tcp),
        ]);
        assert!(
            rules
                .admit_literal("169.254.169.254:80".parse().unwrap(), Protocol::Tcp)
                .is_err()
                && rules
                    .admit_literal("224.0.0.1:80".parse().unwrap(), Protocol::Tcp)
                    .is_err(),
            "Ruleset::admit_literal must refuse a metadata or reserved address an endpoint names"
        );
    }

    // @behavior NW-015
    #[test]
    fn an_http_endpoint_admits_no_raw_tcp() {
        let rules = ruleset(&[("127.0.0.1", 8080, Protocol::Http)]);
        assert!(
            rules
                .admit_literal("127.0.0.1:8080".parse().unwrap(), Protocol::Tcp)
                .is_err(),
            "Ruleset::admit_literal of raw TCP to an http endpoint's address must be refused"
        );
    }
}
