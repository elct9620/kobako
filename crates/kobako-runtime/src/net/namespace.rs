//! The network one Invocation's entries make, as Linux's `struct net` holds the
//! rules every socket of a namespace passes and the families that dial it. The
//! guest sees only virtual addresses: a lookup hands out one for each real
//! address the name resolved to, and a connect to one is translated to the
//! real address it stands for, as cgroup `connect4` rewrites one destination,
//! so no name is ever resolved twice.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex, PoisonError, Weak};

use super::egress::EgressProxy;
use super::family::admitted_addresses_of;
use super::http::{
    split_authority, to_field_name, to_field_value, to_origin_form, Exchange, HttpError, Outbound,
    OutgoingRequest, Scheme,
};
use super::lookup::{normalize, LookupError, NameLookup};
use super::socket::BindHash;
use super::transport::Connector;
use super::{MemoryCharge, OpenConnections, ProtoOps, ProtocolFamily, TcpSock, UdpSock};
use crate::grant::Policy;
use crate::network::{Connection, EntryError, Name, Network, Protocol, Request, Ruleset};
use crate::vfs::{Cred, FilesystemError};

/// The bottom of the virtual range, `198.18.0.0/15`, inside the reserved set,
/// so no real destination is ever in it.
const VIRTUAL_BASE: u32 = u32::from_be_bytes([198, 18, 0, 0]);

/// How many virtual addresses the range holds.
const VIRTUAL_SIZE: u32 = 1 << 17;

/// How many virtual addresses lookups may take: all but the guest's own,
/// `LOCAL_ADDRESS`, and the broadcast one above it.
const MAX_NAMED_ADDRESSES: u32 = VIRTUAL_SIZE - 2;

/// What the guest may reach, which policies judge each lookup and connect, the
/// backend that dials it, the connections the Sandbox holds open and those
/// left to this Invocation, and the virtual addresses handed out.
pub struct Namespace {
    ruleset: Arc<Ruleset>,
    resolve: Vec<Arc<dyn for<'a> Policy<Name<'a>>>>,
    connect: Vec<Arc<dyn for<'a> Policy<Connection<'a>>>>,
    /// The HTTP layer's request policies and bound credentials.
    egress: EgressProxy,
    family: Arc<dyn ProtocolFamily>,
    outbound: Arc<dyn Outbound>,
    open_connections: Arc<OpenConnections>,
    /// How many more connections this Invocation may make; `None` makes no
    /// count.
    connections_left: Mutex<Option<usize>>,
    translations: Mutex<Translations>,
    /// The virtual ports TCP sockets hold, and those UDP sockets hold, as
    /// each protocol keeps its own `bhash`.
    tcp_ports: Arc<BindHash>,
    udp_ports: Arc<BindHash>,
    /// Every TCP socket made here, so an orphan still sending ends with the
    /// Invocation.
    tcp_sockets: Mutex<Vec<Weak<TcpSock>>>,
}

/// Each real address a name looked up resolved to, with the name, at its
/// virtual address's offset from the bottom of the range, and the memory
/// they hold against the Invocation's, given back as the namespace goes.
#[derive(Default)]
struct Translations {
    entries: Vec<(String, IpAddr)>,
    memory: Option<MemoryCharge>,
}

impl Translations {
    /// The offsets of `name`'s entries and the real addresses they hold, in
    /// the order its lookup answered them.
    fn entries_by_name<'a>(&'a self, name: &'a str) -> impl Iterator<Item = (u32, IpAddr)> + 'a {
        (0u32..)
            .zip(&self.entries)
            .filter(move |(_, (known, _))| known == name)
            .map(|(offset, (_, real))| (offset, *real))
    }
}

/// The Invocation's end lets go of every orphan still sending, so nothing it
/// made outlives it.
impl Drop for Namespace {
    fn drop(&mut self) {
        let sockets = std::mem::take(
            self.tcp_sockets
                .get_mut()
                .unwrap_or_else(PoisonError::into_inner),
        );
        for sock in sockets.iter().filter_map(Weak::upgrade) {
            sock.abandon();
        }
    }
}

impl Namespace {
    /// The namespace every entry of `networks` makes together, dialing through
    /// `family` and sending HTTP through `outbound`, refused for an endpoint
    /// the ruleset cannot compile. Each socket, request, and lookup counts
    /// against `open_connections` while open, and the Invocation makes at
    /// most `connection_limit` connections in all.
    pub fn new<'a>(
        networks: impl IntoIterator<Item = &'a Network>,
        family: Arc<dyn ProtocolFamily>,
        outbound: Arc<dyn Outbound>,
        open_connections: Arc<OpenConnections>,
        connection_limit: Option<usize>,
    ) -> Result<Self, EntryError> {
        let networks = networks.into_iter().collect::<Vec<_>>();
        // Each entry's endpoints are read once, so its credentials bind to the
        // very endpoints that judge reach.
        let endpoints = networks
            .iter()
            .map(|network| network.endpoints())
            .collect::<Vec<_>>();
        let ruleset = Ruleset::new(endpoints.iter().flatten().cloned())?;
        let bound = networks
            .iter()
            .zip(&endpoints)
            .map(|(network, endpoints)| (endpoints, network.credentials()))
            .filter(|(_, credentials)| !credentials.is_empty())
            .map(|(endpoints, credentials)| Ok((Ruleset::new(endpoints.clone())?, credentials)))
            .collect::<Result<Vec<_>, EntryError>>()?;
        let policies = networks
            .iter()
            .map(|network| network.policies())
            .collect::<Vec<_>>();
        Ok(Self {
            ruleset: Arc::new(ruleset),
            resolve: policies.iter().filter_map(|p| p.resolve.clone()).collect(),
            connect: policies.iter().filter_map(|p| p.connect.clone()).collect(),
            egress: EgressProxy {
                request: policies.iter().filter_map(|p| p.request.clone()).collect(),
                bound,
            },
            family,
            outbound,
            open_connections,
            connections_left: Mutex::new(connection_limit),
            translations: Mutex::default(),
            tcp_ports: BindHash::new(),
            udp_ports: BindHash::new(),
            tcp_sockets: Mutex::new(Vec::new()),
        })
    }

    /// An HTTP request in flight, as `cred`. The socket hooks judge the
    /// authority the guest named first, a name passing the `resolve` policies
    /// too, and an `http` endpoint must name it at its port; a literal address
    /// passes the ruleset as itself, so a virtual one is refused, and an IPv6
    /// one reaches nothing. A header the guest may not set is refused too, so
    /// the seam frames every request itself, and so is a target not in origin
    /// form, which a proxy would forward past the judged authority. Any
    /// refusal answers `Denied`, and nothing is sent.
    pub fn send(
        &self,
        cred: &Arc<Cred>,
        request: OutgoingRequest,
    ) -> Result<Arc<Exchange>, HttpError> {
        let in_origin_form = to_origin_form(&request.path_with_query).as_deref()
            == Some(request.path_with_query.as_str());
        if !in_origin_form
            || request.headers.iter().any(|(name, value)| {
                to_field_name(name).is_err() || to_field_value(value.clone()).is_err()
            })
        {
            return Err(HttpError::Denied);
        }
        let (host, port) =
            split_authority(&request.authority, request.scheme).ok_or(HttpError::Denied)?;
        let literal = host.parse::<IpAddr>().ok();
        let host = match literal {
            Some(_) => host,
            None => normalize(&host).map_err(|_| HttpError::Denied)?,
        };
        let connection = Connection::new(&host, port, Protocol::Http);
        if self.judge(cred, &connection).is_err() {
            return Err(HttpError::Denied);
        }
        let reachable = match literal {
            Some(ip) => self
                .admit_judged_literal(SocketAddr::new(ip, port), Protocol::Http)
                .is_ok(),
            None => self.allows_lookup(&host),
        };
        let path = request
            .path_with_query
            .split_once('?')
            .map_or(request.path_with_query.as_str(), |(path, _)| path);
        let method = request.method.to_ascii_uppercase();
        let names = request
            .headers
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>();
        let scheme = match request.scheme {
            Scheme::Http => "http",
            Scheme::Https => "https",
        };
        let sent = Request::new((&host, port), &method, scheme, path, &names);
        let admitted = reachable
            && self.ruleset.admits_request(&host, port, &method, path)
            && self.egress.admits(&sent);
        if !admitted {
            return Err(HttpError::Denied);
        }
        let swaps = match request.scheme {
            Scheme::Https => self.egress.swaps_of(&sent, &request.headers),
            Scheme::Http => Vec::new(),
        };
        let mut request = request;
        request.swaps = swaps;
        let connector = Connector::new(host, port, self.ruleset.clone(), self.family.clone());
        self.within_connection_limit(HttpError::ConnectionLimitReached, || {
            Exchange::start(
                self.outbound.clone(),
                request,
                connector,
                &self.open_connections,
                cred,
            )
        })
    }

    /// A lookup of `name`. The name is normalized, then each entry's `resolve`
    /// policy and the endpoints judge it; a refused or unmatched name answers
    /// as NXDOMAIN before any query, as a real one would. A name the
    /// Invocation already translated answers its first real addresses again,
    /// with no query, so a resolver cannot rebind it.
    pub fn resolve(&self, name: &str) -> Result<Arc<NameLookup>, LookupError> {
        let name = normalize(name)?;
        if !self.allows_lookup(&name) || !self.ruleset.admits_name(&name) {
            return Ok(NameLookup::with_answer(
                name,
                Err(LookupError::NameUnresolvable),
            ));
        }
        let translated = self
            .translations
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entries_by_name(&name)
            .map(|(_, real)| real)
            .collect::<Vec<_>>();
        match translated.is_empty() {
            false => Ok(NameLookup::with_answer(name, Ok(translated))),
            true => NameLookup::start(self.family.clone(), name, &self.open_connections),
        }
    }

    /// Whether every `resolve` policy lets `name`, normalized, be looked up.
    fn allows_lookup(&self, name: &str) -> bool {
        let request = Name::new(name);
        self.resolve.iter().all(|policy| policy.allows(&request))
    }

    /// `connect`, made only while the Invocation has a connection left and
    /// counted once it is, as a budget of subrequests is spent;
    /// `refused`, dialing nothing, once none is left.
    pub(crate) fn within_connection_limit<T, E>(
        &self,
        refused: E,
        connect: impl FnOnce() -> Result<T, E>,
    ) -> Result<T, E> {
        let mut left = self
            .connections_left
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if *left == Some(0) {
            return Err(refused);
        }
        let made = connect()?;
        if let Some(n) = left.as_mut() {
            *n -= 1;
        }
        Ok(made)
    }

    /// The virtual ports `protocol`'s sockets hold here.
    pub(crate) fn ports_of(&self, protocol: Protocol) -> &Arc<BindHash> {
        match protocol {
            Protocol::Udp => &self.udp_ports,
            Protocol::Tcp | Protocol::Http => &self.tcp_ports,
        }
    }

    /// Allocate the virtual addresses `lookup`'s answer is handed out at, one
    /// for each real address in the order it answered, from the bottom of the
    /// range and never reused; a name already handed them keeps them, as a
    /// fake-IP pool answers a known host. `None` while the lookup is under
    /// way, its failure once it failed, and `TemporaryFailure` once the range
    /// cannot hold them all or `cred`'s memory cannot hold the entries, which
    /// the table keeps until the Invocation ends.
    pub fn alloc_virtual_addresses(
        &self,
        cred: &Arc<Cred>,
        lookup: &NameLookup,
    ) -> Option<Result<Vec<Ipv4Addr>, LookupError>> {
        let real = match lookup.answer()? {
            Ok(real) => real,
            Err(err) => return Some(Err(err)),
        };
        let to_virtual = |offset: u32| Ipv4Addr::from(VIRTUAL_BASE + offset);
        let mut translations = self
            .translations
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let known = translations
            .entries_by_name(lookup.name())
            .map(|(offset, _)| to_virtual(offset))
            .collect::<Vec<_>>();
        if !known.is_empty() {
            return Some(Ok(known));
        }
        let first = translations.entries.len();
        let Some(end) = u32::try_from(first + real.len())
            .ok()
            .filter(|end| *end <= MAX_NAMED_ADDRESSES)
        else {
            return Some(Err(LookupError::TemporaryFailure));
        };
        let entries = real
            .into_iter()
            .map(|ip| (lookup.name().to_string(), ip))
            .collect::<Vec<_>>();
        let held = entries
            .iter()
            .map(|entry| std::mem::size_of_val(entry) + entry.0.len())
            .sum::<usize>();
        let charged = match &mut translations.memory {
            Some(memory) => memory.resize(memory.bytes() + held),
            None => MemoryCharge::new(cred, held).map(|memory| {
                translations.memory = Some(memory);
            }),
        };
        if charged.is_err() {
            return Some(Err(LookupError::TemporaryFailure));
        }
        translations.entries.extend(entries);
        let first = u32::try_from(first).expect("below the end of the range");
        Some(Ok((first..end).map(to_virtual).collect()))
    }

    /// The name a virtual address stands for and the real address it stands
    /// for.
    fn translate(&self, ip: Ipv4Addr) -> Option<(String, IpAddr)> {
        let offset = u32::from(ip).checked_sub(VIRTUAL_BASE)?;
        self.translations
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entries
            .get(usize::try_from(offset).ok()?)
            .cloned()
    }

    /// Connect the TCP socket `sock` to `addr`, the address the guest named,
    /// as `cred`, as `__sys_connect` does: the socket hooks judge what the
    /// guest named first, then the socket's `->connect` takes it.
    pub fn connect(
        &self,
        cred: &Cred,
        sock: &TcpSock,
        addr: SocketAddr,
    ) -> Result<(), FilesystemError> {
        self.judge_connection_to(cred, addr, Protocol::Tcp)?;
        ProtoOps::connect(sock, self, addr)
    }

    /// The Protocol Family this namespace's sockets reach the host through.
    pub(crate) fn family(&self) -> &Arc<dyn ProtocolFamily> {
        &self.family
    }

    /// The open connections this namespace's sockets count against.
    pub(crate) fn open_connections(&self) -> &Arc<OpenConnections> {
        &self.open_connections
    }

    /// Create a TCP socket connected to nothing for `cred`, which connects
    /// through this namespace's Protocol Family, as `sock_create` does; its
    /// buffers count against `cred`'s memory, as a socket's do against its
    /// creator's memory cgroup.
    pub fn create_tcp_socket(&self, cred: &Arc<Cred>) -> Result<Arc<TcpSock>, FilesystemError> {
        let sock = TcpSock::new(self.family.clone(), cred)?;
        let mut sockets = self
            .tcp_sockets
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        sockets.retain(|sock| sock.strong_count() > 0);
        sockets.push(Arc::downgrade(&sock));
        Ok(sock)
    }

    /// Create a UDP socket connected to nothing for `cred`, as `sock_create`
    /// does; its receive queue counts against `cred`'s memory.
    pub fn create_udp_socket(&self, cred: &Arc<Cred>) -> Result<Arc<UdpSock>, FilesystemError> {
        UdpSock::new(cred)
    }

    /// Bind `sock` to `addr` as `cred`, once the socket hooks pass it, as
    /// `__sys_bind` asks `security_socket_bind` before the protocol's
    /// `->bind`; the host socket takes its own address as it connects.
    pub fn bind(
        &self,
        cred: &Cred,
        sock: &dyn ProtoOps,
        addr: SocketAddr,
    ) -> Result<(), FilesystemError> {
        cred.security_socket_bind(addr)?;
        sock.bind(self, addr)
    }

    /// Connect a UDP socket to `addr`, the one peer the guest named, as
    /// `cred`, through the same socket hooks a TCP connect passes. A socket
    /// naming no peer would send each datagram anywhere, so it is refused.
    pub fn connect_datagram(
        &self,
        cred: &Cred,
        sock: &UdpSock,
        addr: Option<SocketAddr>,
    ) -> Result<(), FilesystemError> {
        let addr = addr.ok_or(FilesystemError::PermissionDenied)?;
        self.judge_connection_to(cred, addr, Protocol::Udp)?;
        ProtoOps::connect(sock, self, addr)
    }

    /// Judge `connection`, what the guest named, through the socket hooks:
    /// the modules `cred` holds, then each entry's connect policy, the first
    /// refusal ending the judging, as Linux's `security_socket_connect` runs
    /// before the protocol's `connect`.
    fn judge(&self, cred: &Cred, connection: &Connection<'_>) -> Result<(), FilesystemError> {
        cred.security_socket_connect(connection)?;
        match self.connect.iter().all(|policy| policy.allows(connection)) {
            true => Ok(()),
            false => Err(FilesystemError::PermissionDenied),
        }
    }

    /// Whether a request may reach `addr`, a literal the socket hooks already
    /// passed: the guest has no IPv6, so an IPv6 one answers `EAFNOSUPPORT`,
    /// as a socket's `connect4` does; any other meets the ruleset as itself.
    fn admit_judged_literal(
        &self,
        addr: SocketAddr,
        protocol: Protocol,
    ) -> Result<(), FilesystemError> {
        if addr.is_ipv6() {
            return Err(FilesystemError::AddressFamilyNotSupported);
        }
        self.ruleset.admit_literal(addr, protocol)
    }

    /// What `addr` stands for when it lies in the virtual range: its name and
    /// real address, or `None` inside the range when no name holds it.
    fn translated(&self, addr: SocketAddr) -> Option<Option<(String, IpAddr)>> {
        match addr.ip() {
            IpAddr::V4(ip) if u32::from(ip) & !(VIRTUAL_SIZE - 1) == VIRTUAL_BASE => {
                Some(self.translate(ip))
            }
            _ => None,
        }
    }

    /// Judge a `protocol` connection to `addr` through the socket hooks, the
    /// name a virtual address stands for in place of the address, as
    /// `security_socket_connect` runs before the protocol's `connect`.
    fn judge_connection_to(
        &self,
        cred: &Cred,
        addr: SocketAddr,
        protocol: Protocol,
    ) -> Result<(), FilesystemError> {
        let host = match self.translated(addr) {
            Some(Some((name, _))) => name,
            _ => addr.ip().to_string(),
        };
        self.judge(cred, &Connection::new(&host, addr.port(), protocol))
    }

    /// What a socket's connect to `addr` reaches once its state check passed,
    /// as cgroup `connect4` runs inside the protocol's `connect`: an IPv6
    /// address answers `EAFNOSUPPORT`, as `inet_stream_connect` refuses one;
    /// a virtual address turns into the name it stands for and the one real
    /// address it stands for, one no name holds answering `EACCES`; any other
    /// stands as itself.
    pub(crate) fn connect4(&self, addr: SocketAddr) -> Result<Destination, FilesystemError> {
        if addr.is_ipv6() {
            return Err(FilesystemError::AddressFamilyNotSupported);
        }
        match self.translated(addr) {
            None => Ok(Destination::Literal(addr)),
            Some(None) => Err(FilesystemError::PermissionDenied),
            Some(Some((name, real))) => Ok(Destination::Named {
                name,
                real,
                port: addr.port(),
            }),
        }
    }

    /// The real address a `protocol` connection to `dest` dials, once the
    /// ruleset lets it, as the filter judges what leaves after the socket took
    /// its port; `EACCES` when it does not.
    pub(crate) fn filter(
        &self,
        dest: &Destination,
        protocol: Protocol,
    ) -> Result<SocketAddr, FilesystemError> {
        match dest {
            Destination::Literal(addr) => {
                self.ruleset.admit_literal(*addr, protocol)?;
                Ok(*addr)
            }
            Destination::Named { name, real, port } => admitted_addresses_of(
                &*self.family,
                &self.ruleset,
                name,
                &[*real],
                *port,
                protocol,
            )
            .pop()
            .ok_or(FilesystemError::PermissionDenied),
        }
    }
}

/// Where a socket's connect leads once cgroup `connect4` ran: the literal
/// address the guest named, or the name a virtual address stands for, the
/// real address it stands for, and the port.
pub(crate) enum Destination {
    Literal(SocketAddr),
    Named {
        name: String,
        real: IpAddr,
        port: u16,
    },
}

#[cfg(test)]
impl Namespace {
    /// The namespace `network` alone makes, dialing through `family` and
    /// sending through `outbound`, with no connection limit; its endpoints
    /// must compile.
    pub(crate) fn for_network(
        network: &Network,
        family: Arc<dyn ProtocolFamily>,
        outbound: Arc<dyn Outbound>,
    ) -> Self {
        Self::new(
            [network],
            family,
            outbound,
            OpenConnections::new(None),
            None,
        )
        .expect("the endpoints compile")
    }
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;
    use std::time::Duration;

    use super::super::family::testing::CountsDials;
    use super::*;
    use crate::network::{Addresses, Endpoint, Entry, Policies};
    use crate::security::prepare_creds_within;
    use crate::vfs::SecurityOperations;

    /// Each connection a policy was asked about.
    type AskedConnections = Arc<Mutex<Vec<(String, u16, Protocol)>>>;

    /// An entry reaching `127.0.0.1:4567` over TCP, its connect policy
    /// recording what it saw and answering `allows`.
    struct RecordsConnects {
        allows: bool,
        seen: AskedConnections,
    }

    impl Entry for RecordsConnects {
        fn endpoints(&self) -> Vec<Endpoint> {
            vec![Endpoint::new("127.0.0.1", 4567)
                .unwrap()
                .with_protocol(Protocol::Tcp)]
        }

        fn policies(&self) -> Policies {
            let (allows, seen) = (self.allows, self.seen.clone());
            Policies::default().with_connect(Arc::new(move |connection: &Connection<'_>| {
                seen.lock().unwrap().push((
                    connection.host.to_string(),
                    connection.port,
                    connection.protocol,
                ));
                allows
            }))
        }
    }

    fn namespace(allows: &[bool]) -> (Namespace, Arc<CountsDials>, AskedConnections) {
        limited_namespace(allows, None, None)
    }

    /// A namespace whose Sandbox holds at most `open` connections at once,
    /// and whose Invocation makes at most `total`.
    fn limited_namespace(
        allows: &[bool],
        open: Option<usize>,
        total: Option<usize>,
    ) -> (Namespace, Arc<CountsDials>, AskedConnections) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let networks = allows
            .iter()
            .enumerate()
            .map(|(index, allows)| {
                let entry = RecordsConnects {
                    allows: *allows,
                    seen: seen.clone(),
                };
                Network::new(format!("entry{index}"), Arc::new(entry)).unwrap()
            })
            .collect::<Vec<_>>();
        let family = Arc::new(CountsDials::default());
        let namespace = Namespace::new(
            &networks,
            family.clone(),
            Arc::new(crate::net::HyperOutbound::new()),
            OpenConnections::new(open),
            total,
        )
        .expect("the endpoints compile");
        (namespace, family, seen)
    }

    /// An http entry for `::1` and `127.0.0.1` with a credential, counting how
    /// often its endpoints are read and its resolve policy is asked; its
    /// connect policy refuses `refused.example`.
    struct CountsReads {
        reads: AtomicUsize,
        resolved: Arc<AtomicUsize>,
        /// Each host the connect policy was asked about.
        judged: Arc<Mutex<Vec<String>>>,
    }

    impl Entry for CountsReads {
        fn endpoints(&self) -> Vec<Endpoint> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            vec![
                Endpoint::new("::1", 4567).unwrap(),
                Endpoint::new("127.0.0.1", 4567).unwrap(),
            ]
        }

        fn policies(&self) -> Policies {
            let resolved = self.resolved.clone();
            let judged = self.judged.clone();
            Policies::default()
                .with_connect(Arc::new(move |connection: &Connection<'_>| {
                    judged.lock().unwrap().push(connection.host.to_string());
                    connection.host != "refused.example"
                }))
                .with_resolve(Arc::new(move |_: &Name<'_>| {
                    resolved.fetch_add(1, Ordering::SeqCst);
                    true
                }))
        }

        fn credentials(&self) -> Vec<crate::network::Credential> {
            vec![crate::network::Credential::new(
                crate::network::Placement::Bearer,
                "kobako-token",
                Arc::new(|_: &crate::network::Request<'_>| None),
            )]
        }
    }

    // @behavior NW-077
    #[test]
    fn a_request_is_judged_once_in_order_and_never_over_ipv6() {
        let entry = Arc::new(CountsReads {
            reads: AtomicUsize::new(0),
            resolved: Arc::default(),
            judged: Arc::default(),
        });
        let network = Network::new("watched", entry.clone()).unwrap();
        let namespace = Namespace::for_network(
            &network,
            Arc::new(CountsDials::default()),
            Arc::new(crate::net::HyperOutbound::new()),
        );
        let cred = prepare_creds_within(&[], None);
        let get = |authority: &str| OutgoingRequest::new("GET", Scheme::Http, authority);
        let hooked = namespace.send(&cred, get("refused.example:4567")).err();
        let ipv6 = namespace.send(&cred, get("[::1]:4567")).err();
        assert_eq!(
            (
                entry.reads.load(Ordering::SeqCst),
                hooked,
                entry.resolved.load(Ordering::SeqCst),
                ipv6,
                entry.judged.lock().unwrap().clone()
            ),
            (
                1,
                Some(HttpError::Denied),
                0,
                Some(HttpError::Denied),
                vec!["refused.example".to_string(), "::1".to_string()]
            ),
            "Namespace::new must read an entry's endpoints once, Namespace::send must ask no \
             resolve policy once a connect hook refused, and must let the connect hook judge an \
             IPv6 authority an endpoint names before refusing it, as a socket's connect does"
        );
    }

    // @behavior NW-022
    #[test]
    fn a_connect_past_the_sandboxs_cap_answers_eagain() {
        let (namespace, family, _) = limited_namespace(&[true], Some(0), None);
        let cred = prepare_creds_within(&[], None);
        let answer = dial(&namespace, &cred, "127.0.0.1:4567".parse().unwrap());
        assert!(
            matches!(answer, Err(FilesystemError::WouldBlock)) && family.dials() == 0,
            "Namespace::connect past the Sandbox's cap must answer EAGAIN, dialing nothing"
        );
    }

    // @behavior NW-059
    #[test]
    fn an_invocation_past_its_connection_limit_answers_eagain() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let mut entry = crate::network::Builder::default();
        entry.endpoint(
            Endpoint::new("127.0.0.1", listener.local_addr().unwrap().port())
                .unwrap()
                .with_protocol(Protocol::Tcp),
        );
        let network = Network::new("db", Arc::new(entry)).unwrap();
        let namespace = Namespace::new(
            [&network],
            Arc::new(crate::net::HostInet),
            Arc::new(crate::net::HyperOutbound::new()),
            OpenConnections::new(None),
            Some(2),
        )
        .unwrap();
        let cred = prepare_creds_within(&[], None);
        let addr = listener.local_addr().unwrap();
        let made = [dial(&namespace, &cred, addr), dial(&namespace, &cred, addr)];
        drop(made);
        let third = dial(&namespace, &cred, addr);
        assert!(
            matches!(third, Err(FilesystemError::WouldBlock)),
            "Namespace::connect past the Invocation's connection limit must answer EAGAIN even \
             once the earlier connections closed"
        );
    }

    // @behavior NW-164
    #[test]
    fn the_invocations_end_lets_go_of_a_socket_still_sending() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let mut entry = crate::network::Builder::default();
        entry.endpoint(
            Endpoint::new("127.0.0.1", listener.local_addr().unwrap().port())
                .unwrap()
                .with_protocol(Protocol::Tcp),
        );
        let network = Network::new("db", Arc::new(entry)).unwrap();
        let namespace = Namespace::new(
            [&network],
            Arc::new(crate::net::HostInet),
            Arc::new(crate::net::HyperOutbound::new()),
            OpenConnections::new(None),
            None,
        )
        .unwrap();
        let cred = prepare_creds_within(&[], None);
        let mut peers = Vec::new();
        // The host may still take what was queued before the release, under
        // load; each try releases a socket filled again until one holds bytes.
        let held = (0..5)
            .map(|_| {
                let sock = dial(&namespace, &cred, listener.local_addr().unwrap()).unwrap();
                peers.push(listener.accept().unwrap().0);
                settle(&sock);
                let mut file = crate::net::sock_alloc_file(sock.clone(), &cred);
                while file.write(&[7; 16 * 1024]).is_ok() {}
                let held = Arc::downgrade(&sock);
                drop((sock, file));
                held
            })
            .find(|held| held.upgrade().is_some());
        let orphaned = held.is_some();
        drop(namespace);
        let held = held.unwrap_or_default();
        // The reactor may hold the socket a moment longer for an event
        // already under way.
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while held.strong_count() > 0 && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(
            (orphaned, held.strong_count() > 0),
            (true, false),
            "a socket released with bytes still queued to send must hold itself while it sends, \
             and the Invocation's namespace dropping must let it go"
        );
    }

    /// A new TCP socket `namespace` connects to `addr` as `cred`.
    fn dial(
        namespace: &Namespace,
        cred: &Arc<Cred>,
        addr: SocketAddr,
    ) -> Result<Arc<TcpSock>, FilesystemError> {
        let sock = namespace.create_tcp_socket(cred)?;
        namespace.connect(cred, &sock, addr).map(|()| sock)
    }

    /// A new UDP socket `namespace` connects to `addr` as `cred`.
    fn dial_datagram(
        namespace: &Namespace,
        cred: &Arc<Cred>,
        addr: Option<SocketAddr>,
    ) -> Result<Arc<UdpSock>, FilesystemError> {
        let sock = namespace.create_udp_socket(cred)?;
        namespace.connect_datagram(cred, &sock, addr).map(|()| sock)
    }

    fn settle(sock: &TcpSock) {
        while matches!(sock.finish_connect(), Err(FilesystemError::WouldBlock)) {
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    // @behavior NW-018
    #[test]
    fn a_connect_policy_refuses_what_the_endpoints_allow() {
        let (namespace, family, seen) = namespace(&[true, false]);
        let cred = prepare_creds_within(&[], None);
        let answer = dial(&namespace, &cred, "127.0.0.1:4567".parse().unwrap());
        assert!(
            matches!(answer, Err(FilesystemError::PermissionDenied)) && family.dials() == 0,
            "Namespace::connect must refuse with EACCES, dialing nothing, once any entry's \
             connect policy refuses"
        );
        assert_eq!(
            seen.lock().unwrap()[0],
            ("127.0.0.1".to_string(), 4567, Protocol::Tcp),
            "a connect policy must see the host the guest named, its port, and its protocol"
        );
    }

    // @behavior NW-018
    #[test]
    fn a_connect_every_policy_allows_dials_the_address() {
        let (namespace, family, _) = namespace(&[true, true]);
        let cred = prepare_creds_within(&[], None);
        let sock = dial(&namespace, &cred, "127.0.0.1:4567".parse().unwrap())
            .expect("every policy allows");
        settle(&sock);
        assert_eq!(
            family.dials(),
            1,
            "Namespace::connect every connect policy allows must dial the address once"
        );
    }

    // @behavior NW-151
    #[test]
    fn the_hooks_judge_what_the_guest_named_before_its_family_and_translation() {
        let (namespace, family, seen) = namespace(&[true]);
        let cred = prepare_creds_within(&[], None);
        let ipv6 = dial(&namespace, &cred, "[::1]:4567".parse().unwrap()).err();
        let unmapped = dial(&namespace, &cred, "198.18.5.5:4567".parse().unwrap()).err();
        let sock = UdpSock::new(&cred).expect("an unbounded socket is made");
        let bound = namespace
            .bind(&cred, sock.as_ref(), "[::]:0".parse().unwrap())
            .err();
        let judged = seen
            .lock()
            .unwrap()
            .iter()
            .map(|(host, _, _)| host.clone())
            .collect::<Vec<_>>();
        assert_eq!(
            (ipv6, unmapped, bound, judged, family.dials()),
            (
                Some(FilesystemError::AddressFamilyNotSupported),
                Some(FilesystemError::PermissionDenied),
                Some(FilesystemError::AddressFamilyNotSupported),
                vec!["::1".to_string(), "198.18.5.5".to_string()],
                0
            ),
            "Namespace::connect must show the connect policy an IPv6 address and an unmapped \
             virtual one before answering EAFNOSUPPORT and EACCES, Namespace::bind must answer \
             EAFNOSUPPORT for an IPv6 address, and none may dial"
        );
    }

    /// A security module refusing every connect.
    struct RefusesConnects;

    impl SecurityOperations for RefusesConnects {
        fn socket_connect(&self, _connection: &Connection<'_>) -> Result<(), FilesystemError> {
            Err(FilesystemError::PermissionDenied)
        }
    }

    // @behavior NW-019
    #[test]
    fn a_security_module_refuses_before_any_entry_policy() {
        let (namespace, family, seen) = namespace(&[true]);
        let cred = prepare_creds_within(&[Arc::new(RefusesConnects)], None);
        let answer = dial(&namespace, &cred, "127.0.0.1:4567".parse().unwrap());
        assert!(
            matches!(answer, Err(FilesystemError::PermissionDenied))
                && seen.lock().unwrap().is_empty()
                && family.dials() == 0,
            "a Wasm Driver's security module refusing socket_connect must refuse before any \
             entry's connect policy is asked, dialing nothing"
        );
    }

    /// A family resolving every name to what `answers` holds, counting each
    /// lookup, recording each address dialed, and holding `local` as the
    /// host's own addresses, unless `local_unreadable` says it cannot read
    /// them.
    #[derive(Default)]
    struct ResolvesNames {
        answers: Mutex<Vec<IpAddr>>,
        lookups: AtomicUsize,
        dials: Mutex<Vec<SocketAddr>>,
        local: Vec<IpAddr>,
        local_unreadable: bool,
    }

    impl ProtocolFamily for ResolvesNames {
        fn connect(&self, addr: SocketAddr) -> io::Result<std::net::TcpStream> {
            self.dials.lock().unwrap().push(addr);
            Err(io::ErrorKind::ConnectionRefused.into())
        }

        fn resolve(&self, _name: &str) -> io::Result<Vec<IpAddr>> {
            self.lookups.fetch_add(1, Ordering::SeqCst);
            Ok(self.answers.lock().unwrap().clone())
        }

        fn local_addresses(&self) -> io::Result<Vec<IpAddr>> {
            match self.local_unreadable {
                true => Err(io::ErrorKind::PermissionDenied.into()),
                false => Ok(self.local.clone()),
            }
        }
    }

    /// A namespace of `endpoints` over `family`, its one entry refusing to
    /// resolve `refused`.
    fn resolving_namespace(
        endpoints: Vec<Endpoint>,
        refused: &'static str,
        family: Arc<ResolvesNames>,
    ) -> Namespace {
        let mut entry = crate::network::Builder::default();
        endpoints.into_iter().for_each(|endpoint| {
            entry.endpoint(endpoint);
        });
        entry.resolve_policy(move |name: &Name<'_>| name.name != refused);
        let network = Network::new("names", Arc::new(entry)).unwrap();
        Namespace::for_network(&network, family, Arc::new(crate::net::HyperOutbound::new()))
    }

    fn tcp(host: &str, port: u16) -> Endpoint {
        Endpoint::new(host, port)
            .unwrap()
            .with_protocol(Protocol::Tcp)
    }

    /// The answer `lookup` settles on.
    fn wait_for_answer(lookup: &NameLookup) -> Result<Vec<IpAddr>, LookupError> {
        loop {
            if let Some(answer) = lookup.answer() {
                return answer;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// The virtual addresses a lookup of `name` hands out, at port 6379.
    fn resolve_virtual_addresses(namespace: &Namespace, name: &str) -> Vec<SocketAddr> {
        let lookup = namespace.resolve(name).expect("the lookup starts");
        wait_for_answer(&lookup).expect("the name resolves");
        namespace
            .alloc_virtual_addresses(&prepare_creds_within(&[], None), &lookup)
            .expect("the lookup settled")
            .expect("the range has room")
            .into_iter()
            .map(|ip| SocketAddr::new(IpAddr::V4(ip), 6379))
            .collect()
    }

    /// The one virtual address a lookup of `name`, answering one address,
    /// hands out.
    fn resolve_virtual_address(namespace: &Namespace, name: &str) -> SocketAddr {
        let [at] = resolve_virtual_addresses(namespace, name)[..] else {
            panic!("a lookup answering one address hands out one virtual address");
        };
        at
    }

    // @behavior NW-024
    #[test]
    fn a_name_no_endpoint_names_or_a_policy_refuses_answers_nxdomain_unasked() {
        let family = Arc::new(ResolvesNames::default());
        let namespace = resolving_namespace(
            vec![tcp("cache.internal", 6379), tcp("*.example.com", 443)],
            "blocked.example.com",
            family.clone(),
        );
        let answers = ["other.internal", "blocked.example.com", "example.com"]
            .map(|name| wait_for_answer(&namespace.resolve(name).unwrap()));
        assert!(
            answers
                .iter()
                .all(|answer| *answer == Err(LookupError::NameUnresolvable))
                && family.lookups.load(Ordering::SeqCst) == 0,
            "Namespace::resolve of a name no endpoint names, one a resolve policy refuses, or a \
             wildcard's own apex must answer name-unresolvable without asking the resolver, got \
             {answers:?}"
        );
    }

    // @behavior NW-154
    #[test]
    fn a_connect_is_refused_when_the_hosts_own_addresses_cannot_be_read() {
        let family = Arc::new(ResolvesNames {
            answers: Mutex::new(vec!["8.8.8.8".parse().unwrap()]),
            local_unreadable: true,
            ..ResolvesNames::default()
        });
        let namespace = resolving_namespace(vec![tcp("api.example.com", 443)], "", family.clone());
        let cred = prepare_creds_within(&[], None);
        let mut at = resolve_virtual_address(&namespace, "api.example.com");
        at.set_port(443);
        let answer = dial(&namespace, &cred, at).err();
        assert_eq!(
            (answer, family.dials.lock().unwrap().len()),
            (Some(FilesystemError::PermissionDenied), 0),
            "Namespace::connect must refuse with EACCES, dialing nothing, when the Protocol \
             Family cannot read the host's own addresses, rather than judge the name's \
             addresses as if the host had none"
        );
    }

    /// A family resolving every name to `8.8.8.8` that does not say the host's
    /// own addresses, counting how often it dials.
    #[derive(Default)]
    struct SaysNoLocalAddresses(AtomicUsize);

    impl ProtocolFamily for SaysNoLocalAddresses {
        fn connect(&self, _addr: SocketAddr) -> io::Result<std::net::TcpStream> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Err(io::ErrorKind::ConnectionRefused.into())
        }

        fn resolve(&self, _name: &str) -> io::Result<Vec<IpAddr>> {
            Ok(vec!["8.8.8.8".parse().unwrap()])
        }
    }

    // @behavior NW-154
    #[test]
    fn a_connect_is_refused_through_a_family_that_does_not_say_the_hosts_own_addresses() {
        let family = Arc::new(SaysNoLocalAddresses::default());
        let mut entry = crate::network::Builder::default();
        entry.endpoint(tcp("api.example.com", 443));
        let network = Network::new("names", Arc::new(entry)).unwrap();
        let namespace = Namespace::for_network(
            &network,
            family.clone(),
            Arc::new(crate::net::HyperOutbound::new()),
        );
        let cred = prepare_creds_within(&[], None);
        let mut at = resolve_virtual_address(&namespace, "api.example.com");
        at.set_port(443);
        let answer = dial(&namespace, &cred, at).err();
        assert_eq!(
            (answer, family.0.load(Ordering::SeqCst)),
            (Some(FilesystemError::PermissionDenied), 0),
            "Namespace::connect through a Protocol Family that leaves the host's own addresses \
             unsaid must refuse with EACCES, dialing nothing, as one that cannot read them does"
        );
    }

    // @behavior NW-154
    #[test]
    fn a_request_is_refused_when_the_hosts_own_addresses_cannot_be_read() {
        let family = Arc::new(ResolvesNames {
            answers: Mutex::new(vec!["8.8.8.8".parse().unwrap()]),
            local_unreadable: true,
            ..ResolvesNames::default()
        });
        let endpoint = Endpoint::new("api.example.com", 80).unwrap();
        let namespace = resolving_namespace(vec![endpoint], "", family.clone());
        let cred = prepare_creds_within(&[], None);
        let exchange = namespace
            .send(
                &cred,
                OutgoingRequest::new("GET", Scheme::Http, "api.example.com:80"),
            )
            .expect("the endpoint names the authority");
        let started = std::time::Instant::now();
        let answer = loop {
            if let Some(answer) = exchange.response() {
                break answer.err();
            }
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "the request never settles"
            );
            std::thread::sleep(Duration::from_millis(1));
        };
        assert_eq!(
            (answer, family.dials.lock().unwrap().len()),
            (Some(HttpError::Denied), 0),
            "an HTTP request must answer Denied, dialing nothing, when the Protocol Family \
             cannot read the host's own addresses"
        );
    }

    // @behavior NW-149
    #[test]
    fn a_virtual_address_is_held_against_the_invocations_memory() {
        let family = Arc::new(ResolvesNames {
            answers: Mutex::new(vec!["127.0.0.1".parse().unwrap()]),
            ..ResolvesNames::default()
        });
        let namespace = resolving_namespace(vec![tcp("cache.internal", 6379)], "", family);
        let lookup = namespace
            .resolve("cache.internal")
            .expect("the lookup starts");
        wait_for_answer(&lookup).expect("the name resolves");
        let full = namespace.alloc_virtual_addresses(&prepare_creds_within(&[], Some(0)), &lookup);
        let roomy = namespace.alloc_virtual_addresses(&prepare_creds_within(&[], None), &lookup);
        assert_eq!(
            (full, roomy),
            (
                Some(Err(LookupError::TemporaryFailure)),
                Some(Ok(vec!["198.18.0.0".parse().unwrap()]))
            ),
            "a lookup must answer temporary-failure when the Invocation's memory cannot hold \
             the name's entry, and hand out the bottom of the range once it can"
        );
    }

    // @behavior NW-175
    #[test]
    fn a_name_keeps_its_virtual_addresses_and_first_answer_for_the_invocation() {
        let family = Arc::new(ResolvesNames {
            answers: Mutex::new(vec![
                "127.0.0.1".parse().unwrap(),
                "127.0.0.2".parse().unwrap(),
            ]),
            ..ResolvesNames::default()
        });
        let refusing = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut entry = crate::network::Builder::default();
        entry.endpoint(tcp("cache.internal", 6379).with_addresses(vec![Addresses::Local]));
        let refuses = refusing.clone();
        entry.resolve_policy(move |_: &Name<'_>| !refuses.load(Ordering::SeqCst));
        let network = Network::new("names", Arc::new(entry)).unwrap();
        let namespace = Namespace::for_network(
            &network,
            family.clone(),
            Arc::new(crate::net::HyperOutbound::new()),
        );
        let first = resolve_virtual_addresses(&namespace, "cache.internal");
        *family.answers.lock().unwrap() = vec!["10.0.0.1".parse().unwrap()];
        let again = namespace
            .resolve("Cache.Internal.")
            .expect("the lookup starts");
        let answer = wait_for_answer(&again);
        let second = resolve_virtual_addresses(&namespace, "cache.internal");
        let cred = prepare_creds_within(&[], None);
        let sock = dial(&namespace, &cred, second[1]).expect("the endpoint leads there");
        while matches!(sock.finish_connect(), Err(FilesystemError::WouldBlock)) {
            std::thread::sleep(Duration::from_millis(1));
        }
        refusing.store(true, Ordering::SeqCst);
        let refused = wait_for_answer(&namespace.resolve("cache.internal").expect("it answers"));
        assert_eq!(
            (
                first,
                second,
                answer,
                refused,
                family.lookups.load(Ordering::SeqCst),
                namespace.translations.lock().unwrap().entries.len(),
                family.dials.lock().unwrap().clone()
            ),
            (
                vec![
                    "198.18.0.0:6379".parse().unwrap(),
                    "198.18.0.1:6379".parse().unwrap()
                ],
                vec![
                    "198.18.0.0:6379".parse().unwrap(),
                    "198.18.0.1:6379".parse().unwrap()
                ],
                Ok(vec![
                    "127.0.0.1".parse().unwrap(),
                    "127.0.0.2".parse().unwrap()
                ]),
                Err(LookupError::NameUnresolvable),
                1,
                2,
                vec!["127.0.0.2:6379".parse().unwrap()]
            ),
            "a name looked up again in one Invocation, in any spelling of it, must answer the \
             same virtual addresses, each standing for the same first real address, query the \
             resolver once, and take no second entries, so a resolver cannot rebind it, while a \
             resolve policy that refuses it later still answers NXDOMAIN"
        );
    }

    // @behavior NW-025
    #[test]
    fn a_name_connects_through_its_virtual_address_to_where_its_endpoint_leads() {
        let family = Arc::new(ResolvesNames {
            answers: Mutex::new(vec![
                "127.0.0.1".parse().unwrap(),
                "10.0.0.1".parse().unwrap(),
            ]),
            ..ResolvesNames::default()
        });
        let local = tcp("cache.internal", 6379).with_addresses(vec![Addresses::Local]);
        let namespace =
            resolving_namespace(vec![local, tcp("api.internal", 6379)], "", family.clone());
        let cred = prepare_creds_within(&[], None);
        let at = resolve_virtual_addresses(&namespace, "Cache.Internal.");
        let sock = dial(&namespace, &cred, at[0]).expect("the endpoint leads there");
        while matches!(sock.finish_connect(), Err(FilesystemError::WouldBlock)) {
            std::thread::sleep(Duration::from_millis(1));
        }
        let private = dial(&namespace, &cred, at[1]).err();
        let public_only = resolve_virtual_addresses(&namespace, "api.internal");
        let local_refused = dial(&namespace, &cred, public_only[0]).err();
        assert_eq!(
            (
                at,
                family.dials.lock().unwrap().clone(),
                private,
                local_refused
            ),
            (
                vec![
                    "198.18.0.0:6379".parse().unwrap(),
                    "198.18.0.1:6379".parse().unwrap()
                ],
                vec!["127.0.0.1:6379".parse().unwrap()],
                Some(FilesystemError::PermissionDenied),
                Some(FilesystemError::PermissionDenied)
            ),
            "a name's lookup must hand out one virtual address for each real address, and a \
             connect to one must dial only the real address it stands for, refusing one its \
             endpoint's addresses do not lead to, as an endpoint leading only to public \
             addresses refuses a local one"
        );
    }

    // @behavior NW-026
    #[test]
    fn a_virtual_address_no_lookup_handed_out_is_refused() {
        let namespace = resolving_namespace(vec![tcp("cache.internal", 6379)], "", Arc::default());
        let cred = prepare_creds_within(&[], None);
        assert!(
            matches!(
                dial(&namespace, &cred, "198.18.0.9:6379".parse().unwrap()),
                Err(FilesystemError::PermissionDenied)
            ),
            "a connect to a virtual address no lookup handed out must be refused as EACCES"
        );
    }

    // @behavior NW-027
    #[test]
    fn a_connect_dials_what_the_lookup_answered_never_resolving_again() {
        let family = Arc::new(ResolvesNames {
            answers: Mutex::new(vec!["10.0.0.1".parse().unwrap()]),
            ..ResolvesNames::default()
        });
        let private = tcp("cache.internal", 6379).with_addresses(vec![Addresses::Private]);
        let namespace = resolving_namespace(vec![private], "", family.clone());
        let at = resolve_virtual_address(&namespace, "cache.internal");
        *family.answers.lock().unwrap() = vec!["127.0.0.1".parse().unwrap()];
        let cred = prepare_creds_within(&[], None);
        let sock = dial(&namespace, &cred, at).expect("the endpoint leads there");
        while matches!(sock.finish_connect(), Err(FilesystemError::WouldBlock)) {
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(
            (
                family.lookups.load(Ordering::SeqCst),
                family.dials.lock().unwrap().clone()
            ),
            (1, vec!["10.0.0.1:6379".parse().unwrap()]),
            "a connect to a virtual address must dial the address its lookup answered, never \
             resolving the name again"
        );
    }

    // @behavior NW-028
    #[test]
    fn the_hosts_own_address_is_local_whatever_registry_classes_it() {
        let own: IpAddr = "93.184.216.34".parse().unwrap();
        let family = Arc::new(ResolvesNames {
            answers: Mutex::new(vec![own]),
            local: vec![own],
            ..ResolvesNames::default()
        });
        let namespace = resolving_namespace(vec![tcp("api.example.com", 6379)], "", family.clone());
        let at = resolve_virtual_address(&namespace, "api.example.com");
        let cred = prepare_creds_within(&[], None);
        assert!(
            matches!(
                dial(&namespace, &cred, at),
                Err(FilesystemError::PermissionDenied)
            ) && family.dials.lock().unwrap().is_empty(),
            "a name resolving to the host's own public address must be refused by an endpoint \
             leading only to public addresses, since the host's addresses are local"
        );
    }

    // @behavior NW-054
    #[test]
    fn a_datagram_stream_passes_only_a_udp_endpoint() {
        let peer = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let port = peer.local_addr().unwrap().port();
        let udp = Endpoint::new("127.0.0.1", port)
            .unwrap()
            .with_protocol(Protocol::Udp);
        let tcp_only = tcp("127.0.0.2", port);
        let network = Network::new(
            "dgram",
            Arc::new({
                let mut entry = crate::network::Builder::default();
                entry.endpoint(udp).endpoint(tcp_only);
                entry
            }),
        )
        .unwrap();
        let namespace = Namespace::for_network(
            &network,
            Arc::new(crate::net::HostInet),
            Arc::new(crate::net::HyperOutbound::new()),
        );
        let cred = prepare_creds_within(&[], None);
        let admitted = dial_datagram(&namespace, &cred, Some(peer.local_addr().unwrap()));
        let refused = dial_datagram(
            &namespace,
            &cred,
            Some(format!("127.0.0.2:{port}").parse().unwrap()),
        );
        if let Ok(sock) = &admitted {
            crate::net::ProtoOps::release(&**sock);
        }
        assert!(
            admitted.is_ok() && matches!(refused, Err(FilesystemError::PermissionDenied)),
            "Namespace::connect_datagram must pass an address a udp endpoint names and refuse one \
             only a tcp endpoint names"
        );
    }
}
