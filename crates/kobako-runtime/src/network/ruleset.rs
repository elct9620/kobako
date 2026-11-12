//! What the guest may reach, compiled for one Invocation from every entry's
//! endpoints, as an nftables ruleset with a default policy of drop. The fixed
//! sets, `metadata` and `reserved`, are kobako's own and sit in no entry, so no
//! setting reaches them.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use super::{Addresses, Endpoint, EntryError, HttpRule, Protocol};
use crate::Errno;

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

fn is_within(ip: Ipv4Addr, net: [u8; 4], prefix: u32) -> bool {
    let mask = u32::MAX.checked_shl(32 - prefix).unwrap_or(0);
    u32::from(ip) & mask == u32::from(Ipv4Addr::from(net)) & mask
}

fn is_within_v6(ip: Ipv6Addr, net: Ipv6Addr, prefix: u32) -> bool {
    let mask = u128::MAX.checked_shl(128 - prefix).unwrap_or(0);
    u128::from(ip) & mask == u128::from(net) & mask
}

/// The IPv4 address an IPv6 one carries, as a mapped, compatible, or NAT64
/// address does, so each is judged as the IPv4 inside. NAT64 takes both the
/// well-known prefix and the local-use one at its /96, as RFC 8215 lets a
/// DNS64 synthesize either.
fn to_v4(ip: Ipv6Addr) -> Option<Ipv4Addr> {
    if let Some(v4) = ip.to_ipv4_mapped() {
        return Some(v4);
    }
    let nat64 = Ipv6Addr::new(0x64, 0xff9b, 0, 0, 0, 0, 0, 0);
    let local_nat64 = Ipv6Addr::new(0x64, 0xff9b, 1, 0, 0, 0, 0, 0);
    let compatible = Ipv6Addr::UNSPECIFIED;
    let carried = Ipv4Addr::from((u128::from(ip) & 0xffff_ffff) as u32);
    match is_within_v6(ip, nat64, 96)
        || is_within_v6(ip, local_nat64, 96)
        || (is_within_v6(ip, compatible, 96) && u32::from(carried) > 1)
    {
        true => Some(carried),
        false => None,
    }
}

/// The set `ip` falls in, judging an IPv6 address carrying an IPv4 one as
/// that IPv4 address.
pub fn address_set_of(ip: IpAddr) -> AddressSet {
    match ip {
        IpAddr::V4(ip) => v4_set_of(ip),
        IpAddr::V6(ip) => match to_v4(ip) {
            Some(v4) => v4_set_of(v4),
            None => v6_set_of(ip),
        },
    }
}

fn v4_set_of(ip: Ipv4Addr) -> AddressSet {
    let globally_reachable = [
        ([192, 0, 0, 9], 32),
        ([192, 0, 0, 10], 32),
        ([192, 31, 196, 0], 24),
        ([192, 52, 193, 0], 24),
        ([192, 175, 48, 0], 24),
    ];
    if METADATA_V4.contains(&ip) || is_within(ip, [169, 254, 0, 0], 16) {
        AddressSet::Metadata
    } else if is_within(ip, [127, 0, 0, 0], 8) || is_within(ip, [0, 0, 0, 0], 8) {
        AddressSet::Local
    } else if is_within(ip, [10, 0, 0, 0], 8)
        || is_within(ip, [172, 16, 0, 0], 12)
        || is_within(ip, [192, 168, 0, 0], 16)
        || is_within(ip, [100, 64, 0, 0], 10)
    {
        AddressSet::Private
    } else if globally_reachable
        .iter()
        .any(|(net, prefix)| is_within(ip, *net, *prefix))
    {
        AddressSet::Public
    } else if is_within(ip, [192, 0, 0, 0], 24)
        || is_within(ip, [192, 0, 2, 0], 24)
        || is_within(ip, [192, 88, 99, 0], 24)
        || is_within(ip, [198, 18, 0, 0], 15)
        || is_within(ip, [198, 51, 100, 0], 24)
        || is_within(ip, [203, 0, 113, 0], 24)
        || is_within(ip, [224, 0, 0, 0], 4)
        || is_within(ip, [240, 0, 0, 0], 4)
    {
        AddressSet::Reserved
    } else {
        AddressSet::Public
    }
}

/// An IPv6 address's set. Only global unicast, `2000::/3`, is routed, so any
/// other address is reserved; within it, each block IANA's registry says no
/// route reaches is reserved, save the ones inside `2001::/23` it marks
/// globally reachable.
fn v6_set_of(ip: Ipv6Addr) -> AddressSet {
    let net = |a: u16, b: u16, c: u16| Ipv6Addr::new(a, b, c, 0, 0, 0, 0, 0);
    let globally_reachable = [
        (Ipv6Addr::new(0x2001, 1, 0, 0, 0, 0, 0, 1), 128),
        (Ipv6Addr::new(0x2001, 1, 0, 0, 0, 0, 0, 2), 128),
        (Ipv6Addr::new(0x2001, 1, 0, 0, 0, 0, 0, 3), 128),
        (net(0x2001, 3, 0), 32),
        (net(0x2001, 4, 0x112), 48),
        (net(0x2001, 0x20, 0), 28),
        (net(0x2001, 0x30, 0), 28),
    ];
    if METADATA_V6.contains(&ip) || is_within_v6(ip, net(0xfe80, 0, 0), 10) {
        AddressSet::Metadata
    } else if ip == Ipv6Addr::LOCALHOST || ip == Ipv6Addr::UNSPECIFIED {
        AddressSet::Local
    } else if is_within_v6(ip, net(0xfc00, 0, 0), 7) {
        AddressSet::Private
    } else if !is_within_v6(ip, net(0x2000, 0, 0), 3)
        || is_within_v6(ip, net(0x2001, 0xdb8, 0), 32)
        || is_within_v6(ip, net(0x2002, 0, 0), 16)
        || is_within_v6(ip, net(0x3fff, 0, 0), 20)
        || (is_within_v6(ip, net(0x2001, 0, 0), 23)
            && !globally_reachable
                .iter()
                .any(|(block, prefix)| is_within_v6(ip, *block, *prefix)))
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
    Cidr(IpAddr, u8),
}

fn parse_host(host: &str) -> Result<Host, EntryError> {
    if let Ok(ip) = host.parse::<IpAddr>() {
        return Ok(Host::Cidr(ip, full_prefix_of(ip)));
    }
    if let Some((ip, prefix)) = host.split_once('/') {
        let ip = ip
            .parse::<IpAddr>()
            .map_err(|_| EntryError(format!("an endpoint's prefix is no address: {host}")))?;
        let prefix = prefix
            .parse::<u8>()
            .ok()
            .filter(|p| *p <= full_prefix_of(ip))
            .ok_or_else(|| EntryError(format!("an endpoint's prefix is out of range: {host}")))?;
        return Ok(Host::Cidr(ip, prefix));
    }
    // A name is judged as its A-label, the form the guest's lookup is judged
    // in, so an IDN an endpoint names matches the name the guest asks for.
    let a_label = |name: &str| {
        to_a_label(name)
            .ok_or_else(|| EntryError(format!("an endpoint's name has no A-label: {host}")))
    };
    match host.strip_prefix("*.") {
        Some(suffix) if !suffix.is_empty() => Ok(Host::Wildcard(a_label(suffix)?)),
        _ => Ok(Host::Name(a_label(host)?)),
    }
}

/// `name` as the resolver, the hooks, and the endpoints judge it: lowercase,
/// with no trailing dot for the root, and an IDN as its A-label; `None` when no
/// A-label spells it, as for a name left ending in an empty label.
pub(crate) fn to_a_label(name: &str) -> Option<String> {
    let name = name.strip_suffix('.').unwrap_or(name);
    idna::domain_to_ascii(name)
        .ok()
        .filter(|ascii| !ascii.is_empty() && !ascii.ends_with('.'))
}

/// The address an endpoint judges `ip` as, the IPv4 one an IPv6 address
/// carries in its place; `None` for an address in a fixed set, which no
/// endpoint opens.
fn to_screened(ip: IpAddr) -> Option<IpAddr> {
    if matches!(
        address_set_of(ip),
        AddressSet::Metadata | AddressSet::Reserved
    ) {
        return None;
    }
    Some(match ip {
        IpAddr::V6(v6) => to_v4(v6).map_or(ip, IpAddr::V4),
        v4 => v4,
    })
}

/// The longest prefix an address of `ip`'s family takes.
fn full_prefix_of(ip: IpAddr) -> u8 {
    match ip {
        IpAddr::V4(_) => 32,
        IpAddr::V6(_) => 128,
    }
}

/// Whether `ip` is within `net` at `prefix`, an address of another family
/// never, read in `is_within`'s order.
fn is_within_net(ip: IpAddr, net: IpAddr, prefix: u8) -> bool {
    match (net, ip) {
        (IpAddr::V4(net), IpAddr::V4(ip)) => is_within(ip, net.octets(), u32::from(prefix)),
        (IpAddr::V6(net), IpAddr::V6(ip)) => is_within_v6(ip, net, u32::from(prefix)),
        _ => false,
    }
}

struct Rule {
    host: Host,
    port: u16,
    protocol: Protocol,
    addresses: Vec<Addresses>,
    rules: Vec<HttpRule>,
}

/// Whether `path` reads as one path to every server: a `.` or `..` segment,
/// an empty one inside the path, an encoded slash or backslash, an encoded
/// unreserved character, a backslash, or a `;` lets servers normalize or split
/// it differently (RFC 3986 § 5.2.4, § 6.2.2.2), so a rule matching one
/// spelling would pass another.
fn is_unambiguous(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    !(path
        .split('/')
        .any(|segment| segment == "." || segment == "..")
        || path.contains("//")
        || ["%2f", "%5c", "\\", ";"]
            .iter()
            .any(|sign| lower.contains(sign))
        || lower
            .split('%')
            .skip(1)
            .any(|escape| escape.get(..2).is_some_and(encodes_unreserved)))
}

/// Whether the two hex digits `hex` encode a character RFC 3986 calls
/// unreserved, which a server may decode before it matches.
fn encodes_unreserved(hex: &str) -> bool {
    u8::from_str_radix(hex, 16)
        .is_ok_and(|byte| byte.is_ascii_alphanumeric() || b"-._~".contains(&byte))
}

impl Rule {
    /// Whether this rule speaks `protocol` at `port`.
    fn serves(&self, port: u16, protocol: Protocol) -> bool {
        self.port == port && self.protocol == protocol
    }

    /// Whether this rule's host holds `host`, a name or a literal address.
    fn holds(&self, host: &str) -> bool {
        match host.parse::<IpAddr>() {
            Ok(ip) => {
                matches!(self.host, Host::Cidr(net, prefix) if is_within_net(ip, net, prefix))
            }
            Err(_) => self.names(host),
        }
    }

    /// Whether this endpoint's rules allow `method` on `path`: with none,
    /// every request; with some, one allowing it exactly and none denying it
    /// in any spelling a lenient server routes the same.
    fn allows_request(&self, method: &str, path: &str) -> bool {
        self.rules.is_empty()
            || (is_unambiguous(path)
                && self
                    .rules
                    .iter()
                    .any(|rule| rule.allows && rule.matches(method, path))
                && !self
                    .rules
                    .iter()
                    .any(|rule| !rule.allows && rule.matches_leniently(method, path)))
    }

    /// Whether this rule's host names `name`, exactly or beneath its wildcard
    /// on a label boundary.
    fn names(&self, name: &str) -> bool {
        match &self.host {
            Host::Name(host) => host == name,
            Host::Wildcard(suffix) => name
                .strip_suffix(suffix.as_str())
                .is_some_and(|head| head.len() > 1 && head.ends_with('.')),
            Host::Cidr(..) => false,
        }
    }

    /// Whether `ip`, a real address the endpoint's host resolved to, falls
    /// where this rule's `addresses` let it lead. `local` holds the host's own
    /// addresses, which are local whatever registry classes them.
    fn leads_to(&self, ip: IpAddr, local: &[IpAddr]) -> bool {
        let own = local.contains(&ip);
        self.addresses.iter().any(|addresses| match addresses {
            Addresses::Public => !own && address_set_of(ip) == AddressSet::Public,
            Addresses::Private => !own && address_set_of(ip) == AddressSet::Private,
            Addresses::Local => own || address_set_of(ip) == AddressSet::Local,
            Addresses::Cidr(net, prefix) => is_within_net(ip, *net, *prefix),
        })
    }
}

/// Every endpoint one Invocation's entries let the guest reach.
pub struct Ruleset {
    rules: Vec<Rule>,
}

impl Ruleset {
    /// The ruleset of `endpoints`, refused for an endpoint whose host names no
    /// address, prefix, or name, and for a wildcard leading to `private` or
    /// `local`, where a guest-controlled name beneath it could reach.
    pub fn new(endpoints: impl IntoIterator<Item = Endpoint>) -> Result<Self, EntryError> {
        let rules = endpoints
            .into_iter()
            .map(|endpoint| {
                let host = parse_host(&endpoint.host)?;
                if matches!(host, Host::Wildcard(_))
                    && endpoint
                        .addresses
                        .iter()
                        .any(|a| matches!(a, Addresses::Private | Addresses::Local))
                {
                    return Err(EntryError(format!(
                        "a wildcard endpoint may lead only to public addresses or a CIDR: {}",
                        endpoint.host
                    )));
                }
                if endpoint.addresses.iter().any(|a| match a {
                    Addresses::Cidr(ip, prefix) => *prefix > full_prefix_of(*ip),
                    _ => false,
                }) {
                    return Err(EntryError(format!(
                        "an endpoint's addresses hold a prefix out of range: {}",
                        endpoint.host
                    )));
                }
                Ok(Rule {
                    host,
                    port: endpoint.port,
                    protocol: endpoint.protocol,
                    addresses: endpoint.addresses,
                    rules: endpoint.rules,
                })
            })
            .collect::<Result<_, EntryError>>()?;
        Ok(Self { rules })
    }

    /// Whether any endpoint's host names `name`, so a lookup of it may run.
    pub fn admits_name(&self, name: &str) -> bool {
        self.rules.iter().any(|rule| rule.names(name))
    }

    /// Whether a credential of these endpoints' entry is bound to a request to
    /// `host` at `port` by `method` on `path`: an `http` endpoint names that
    /// exact host, never through a wildcard or an address, and allows the
    /// request, on a path every server reads alike. The scheme is the caller's
    /// to hold to `https`.
    pub fn binds(&self, host: &str, port: u16, method: &str, path: &str) -> bool {
        is_unambiguous(path)
            && self.rules.iter().any(|rule| {
                rule.serves(port, Protocol::Http)
                    && matches!(&rule.host, Host::Name(name) if name == host)
                    && rule.allows_request(method, path)
            })
    }

    /// Whether an `http` endpoint holding `host` at `port` allows `method` on
    /// `path`, the path with no query: endpoints join, so any one allowing it
    /// admits the request.
    pub fn admits_request(&self, host: &str, port: u16, method: &str, path: &str) -> bool {
        self.rules.iter().any(|rule| {
            rule.serves(port, Protocol::Http)
                && rule.holds(host)
                && rule.allows_request(method, path)
        })
    }

    /// The addresses at `port`, of `real` that the name the guest looked up
    /// resolved to, it may open a `protocol` connection to, in their order;
    /// the host's own addresses, `local`, count as local. The fixed sets
    /// refuse whatever an endpoint says; otherwise an endpoint naming the name
    /// at that port and protocol must lead to the address, so another allowed
    /// name answering the same address never borrows it.
    pub fn admitted_addresses_of(
        &self,
        name: &str,
        real: &[IpAddr],
        port: u16,
        protocol: Protocol,
        local: &[IpAddr],
    ) -> Vec<SocketAddr> {
        real.iter()
            .map(|ip| SocketAddr::new(*ip, port))
            .filter(|addr| self.admits_resolved(name, *addr, protocol, local))
            .collect()
    }

    fn admits_resolved(
        &self,
        name: &str,
        addr: SocketAddr,
        protocol: Protocol,
        local: &[IpAddr],
    ) -> bool {
        let Some(ip) = to_screened(addr.ip()) else {
            return false;
        };
        self.rules.iter().any(|rule| {
            rule.serves(addr.port(), protocol) && rule.names(name) && rule.leads_to(ip, local)
        })
    }

    /// Whether the guest may open a `protocol` connection to `addr`, the
    /// literal address it named, as nftables judges each address: refused
    /// unless an endpoint with an address host holds it, and refused in the
    /// fixed sets whatever an endpoint says. An `http` endpoint never admits
    /// raw TCP, so its rules cannot be framed around.
    pub fn admit_literal(&self, addr: SocketAddr, protocol: Protocol) -> Result<(), Errno> {
        let ip = to_screened(addr.ip()).ok_or(Errno::PermissionDenied)?;
        let matched = self.rules.iter().any(|rule| {
            rule.serves(addr.port(), protocol)
                && matches!(rule.host, Host::Cidr(net, prefix) if is_within_net(ip, net, prefix))
        });
        match matched {
            true => Ok(()),
            false => Err(Errno::PermissionDenied),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // @behavior NW-012
    #[test]
    fn each_address_falls_in_its_set() {
        let set = |ip: &str| address_set_of(ip.parse().unwrap());
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
                set("64:ff9b:1::a9fe:a9fe"),
                set("64:ff9b:1:1::1"),
                set("2001:2::1"),
                set("2001:20::1"),
                set("3fff::1"),
                set("5f00::1"),
                set("fec0::1"),
                set("2606:4700::1111"),
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
                AddressSet::Metadata,
                AddressSet::Reserved,
                AddressSet::Reserved,
                AddressSet::Public,
                AddressSet::Reserved,
                AddressSet::Reserved,
                AddressSet::Reserved,
                AddressSet::Public,
            ],
            "address_set_of must class each address by IANA's registries, judging an IPv6 address \
             carrying an IPv4 one as the IPv4 inside, and an IPv6 address public only within \
             2000::/3 and outside every block the registry says no route reaches"
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

    // @behavior NW-075
    #[test]
    fn an_endpoint_names_an_idn_by_its_a_label() {
        let rules = Ruleset::new([
            Endpoint::new("bücher.example", 443).unwrap(),
            Endpoint::new("*.例え.jp", 443).unwrap(),
            Endpoint::new("rooted.example.", 443).unwrap(),
        ])
        .unwrap();
        let empty_label = Ruleset::new([Endpoint::new("empty.example..", 443).unwrap()]);
        assert_eq!(
            [
                rules.admits_name("xn--bcher-kva.example"),
                rules.admits_name("www.xn--r8jz45g.jp"),
                rules.admits_name(&to_a_label("Rooted.Example.").unwrap()),
                empty_label.is_err(),
                to_a_label("empty.example..").is_none(),
            ],
            [true, true, true, true, true],
            "Ruleset::admits_name must match an endpoint written as an IDN, and a wildcard over \
             one, against the A-label the guest's lookup asks for; an endpoint and a lookup must \
             drop the root's one trailing dot alike, and both refuse a name left ending in an \
             empty label"
        );
    }

    // @behavior NW-029
    #[test]
    fn a_wildcard_leading_to_private_or_local_addresses_is_refused() {
        let wildcard = |addresses| {
            Ruleset::new([Endpoint::new("*.example.com", 443)
                .unwrap()
                .with_addresses(addresses)])
            .is_ok()
        };
        assert_eq!(
            [
                wildcard(vec![Addresses::Private]),
                wildcard(vec![Addresses::Local]),
                wildcard(vec![
                    Addresses::Public,
                    Addresses::Cidr("10.0.0.0".parse().unwrap(), 8)
                ]),
            ],
            [false, false, true],
            "Ruleset::new must refuse a wildcard endpoint leading to private or local addresses, \
             and keep one leading to public addresses or a CIDR"
        );
    }

    // @behavior NW-150
    #[test]
    fn an_address_prefix_out_of_range_is_refused() {
        let leading_to = |ip: &str, prefix| {
            Ruleset::new([Endpoint::new("api.example.com", 443)
                .unwrap()
                .with_addresses(vec![Addresses::Cidr(ip.parse().unwrap(), prefix)])])
            .is_ok()
        };
        assert_eq!(
            [
                leading_to("10.0.0.0", 32),
                leading_to("10.0.0.0", 40),
                leading_to("fd00::", 128),
                leading_to("fd00::", 129),
            ],
            [true, false, true, false],
            "Ruleset::new must refuse an endpoint whose addresses hold a CIDR prefix longer than \
             its family's address, and keep one at the full length"
        );
    }

    // @behavior NW-044
    #[test]
    fn an_http_endpoints_rules_admit_only_the_methods_and_paths_they_allow() {
        let rules = Ruleset::new([
            Endpoint::new("api.example.com", 443)
                .unwrap()
                .with_rules(vec![
                    HttpRule::allow(Some("get"), "/repos/**"),
                    HttpRule::allow(None, "/status"),
                    HttpRule::deny(None, "/repos/*/secrets"),
                ]),
            Endpoint::new("open.example.com", 443).unwrap(),
        ])
        .unwrap();
        let admit = |host, method, path| rules.admits_request(host, 443, method, path);
        assert_eq!(
            [
                admit("api.example.com", "GET", "/repos/a/b"),
                admit("api.example.com", "POST", "/repos/a/b"),
                admit("api.example.com", "DELETE", "/status"),
                admit("api.example.com", "GET", "/repos/a/secrets"),
                admit("api.example.com", "GET", "/repos/../admin"),
                admit("api.example.com", "GET", "/repos/a%2fb"),
                admit("api.example.com", "GET", "/repos/a/./secrets"),
                admit("api.example.com", "GET", "/repos/a//secrets"),
                admit("api.example.com", "GET", "/repos/a/%73ecrets"),
                admit("api.example.com", "GET", "/repos/a/b%20c/"),
                admit("api.example.com", "GET", "/repos/a/secrets/"),
                admit("api.example.com", "GET", "/repos/a/SECRETS"),
                admit("api.example.com", "GET", "/STATUS"),
                admit("open.example.com", "PUT", "/anything/at/all"),
            ],
            [
                true, false, true, false, false, false, false, false, false, true, false, false,
                false, true
            ],
            "Ruleset::admits_request must pass only what an http endpoint's rules allow and none \
             denies, refuse a path servers may split or normalize differently (a . segment, an \
             empty one, an encoded unreserved character) while passing an encoded space and a \
             trailing slash, refuse a denied path however a lenient server spells it (a trailing \
             slash, another case) while an allow matches exactly, and pass any request to an \
             endpoint with no rules"
        );
    }
}
