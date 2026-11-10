//! The network one Invocation's entries make, as Linux's `struct net` holds the
//! rules every socket of a namespace passes and the families that dial it. The
//! guest sees only virtual addresses: a lookup hands out one, and a connect to
//! it is translated to the real addresses the name resolved to, as cgroup
//! `connect4` rewrites a destination, so no name is ever resolved twice.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex, PoisonError};

use super::http::{
    carries, split_authority, to_field_name, to_field_value, Connector, Exchange, HttpError,
    Outbound, OutgoingRequest, Scheme, Swap,
};
use super::lookup::{normalize, LookupError, NameLookup};
use super::{OpenConnections, ProtocolFamily, TcpSock, UdpSock};
use crate::grant::Policy;
use crate::network::{
    Connection, Credential, EntryError, Name, Network, Protocol, Request, Ruleset,
};
use crate::vfs::{Cred, FilesystemError};

/// The bottom of the virtual range, `198.18.0.0/15`, inside the reserved set,
/// so no real destination is ever in it.
const VIRTUAL_BASE: u32 = u32::from_be_bytes([198, 18, 0, 0]);

/// How many virtual addresses the range holds.
const VIRTUAL_SIZE: u32 = 1 << 17;

/// The guest's own end of every socket it connects, kept out of what a lookup
/// hands out, as a container in its own network namespace reads its veth's
/// address while the host translates. It sits below the range's last
/// address, the one a broadcast would take.
const LOCAL_ADDRESS: Ipv4Addr = Ipv4Addr::new(198, 19, 255, 254);

/// How many virtual addresses a lookup may take: all but the guest's own and
/// the broadcast one above it.
const NAMED_SIZE: u32 = VIRTUAL_SIZE - 2;

/// The ports a connect gives the guest for its own end, Linux's default
/// `ip_local_port_range`, taken in turn from the bottom and around again, so
/// no host port reaches the guest and a script reads the same ports each run.
const VIRTUAL_PORTS: std::ops::RangeInclusive<u16> = 32768..=60999;

/// What the guest may reach, which policies judge each lookup and connect, the
/// backend that dials it, the connections the Sandbox holds open and those
/// left to this Invocation, and the virtual addresses handed out.
pub struct Namespace {
    ruleset: Arc<Ruleset>,
    resolve: Vec<Arc<dyn for<'a> Policy<Name<'a>>>>,
    connect: Vec<Arc<dyn for<'a> Policy<Connection<'a>>>>,
    request: Vec<Arc<dyn for<'a> Policy<Request<'a>>>>,
    /// Each entry's own endpoints and the credentials bound to them.
    bound: Vec<(Ruleset, Vec<Credential>)>,
    family: Arc<dyn ProtocolFamily>,
    outbound: Arc<dyn Outbound>,
    open_connections: Arc<OpenConnections>,
    /// How many more connections this Invocation may make; `None` makes no
    /// count.
    connections_left: Mutex<Option<usize>>,
    /// Each name looked up and its real addresses, at its virtual address's
    /// offset from the bottom of the range.
    virtuals: Mutex<Vec<(String, Vec<IpAddr>)>>,
    /// The virtual port this Invocation's next connect takes.
    next_port: Mutex<u16>,
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
            request: policies.iter().filter_map(|p| p.request.clone()).collect(),
            bound,
            family,
            outbound,
            open_connections,
            connections_left: Mutex::new(connection_limit),
            virtuals: Mutex::new(Vec::new()),
            next_port: Mutex::new(*VIRTUAL_PORTS.start()),
        })
    }

    /// An HTTP request in flight, as `cred`. The socket hooks judge the
    /// authority the guest named first, a name passing the `resolve` policies
    /// too, and an `http` endpoint must name it at its port; a literal address
    /// passes the ruleset as itself, so a virtual one is refused, and an IPv6
    /// one reaches nothing. A header the guest may not set is refused too, so
    /// the seam frames every request itself. Any refusal answers `Denied`, and
    /// nothing is sent.
    pub fn send(
        &self,
        cred: &Arc<Cred>,
        request: OutgoingRequest,
    ) -> Result<Arc<Exchange>, HttpError> {
        if request.headers.iter().any(|(name, value)| {
            to_field_name(name).is_err() || to_field_value(value.clone()).is_err()
        }) {
            return Err(HttpError::Denied);
        }
        let (host, port) =
            split_authority(&request.authority, request.scheme).ok_or(HttpError::Denied)?;
        let literal = host.parse::<IpAddr>().ok();
        // The guest has no IPv6, so an IPv6 literal reaches nothing, as its
        // sockets answer EAFNOSUPPORT.
        if literal.is_some_and(|ip| ip.is_ipv6()) {
            return Err(HttpError::Denied);
        }
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
                .ruleset
                .admit_literal(SocketAddr::new(ip, port), Protocol::Http)
                .is_ok(),
            None => self
                .resolve
                .iter()
                .all(|policy| policy.allows(&Name::new(&host))),
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
            && self.request.iter().all(|policy| policy.allows(&sent));
        if !admitted {
            return Err(HttpError::Denied);
        }
        let swaps = match request.scheme {
            Scheme::Https => self.swaps_of(&sent, &request.headers),
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

    /// The credentials bound to `sent`, a request over `https`: an entry's own
    /// endpoint names its exact host and allows it, and the placement's header
    /// carries the placeholder. Each source answers here, on the guest thread,
    /// so its time counts against the deadline.
    fn swaps_of(&self, sent: &Request<'_>, headers: &[(String, Vec<u8>)]) -> Vec<Swap> {
        let (host, port) = sent.authority;
        let mut swaps = Vec::new();
        for (rules, credentials) in &self.bound {
            if !rules.binds(host, port, sent.method, sent.path) {
                continue;
            }
            for credential in credentials {
                let header = credential.placement.header();
                let placeholder = credential.placeholder.as_bytes();
                let carried = headers.iter().any(|(name, value)| {
                    name.eq_ignore_ascii_case(header) && carries(value, placeholder)
                });
                if !carried {
                    continue;
                }
                if let Some(secret) = credential.source.value(sent) {
                    swaps.push(Swap {
                        header: header.to_string(),
                        placeholder: placeholder.to_vec(),
                        secret,
                    });
                }
            }
        }
        swaps
    }

    /// A lookup of `name`. The name is normalized, then each entry's `resolve`
    /// policy and the endpoints judge it; a refused or unmatched name answers
    /// as NXDOMAIN before any query, as a real one would.
    pub fn resolve(&self, name: &str) -> Result<Arc<NameLookup>, LookupError> {
        let name = normalize(name)?;
        let request = Name::new(&name);
        if !self.resolve.iter().all(|policy| policy.allows(&request))
            || !self.ruleset.admits_name(&name)
        {
            return Ok(NameLookup::with_answer(
                name,
                Err(LookupError::NameUnresolvable),
            ));
        }
        NameLookup::start(self.family.clone(), name, &self.open_connections)
    }

    /// `connect`, made only while the Invocation has a connection left and
    /// counted once it is, as a budget of subrequests is spent;
    /// `refused`, dialing nothing, once none is left.
    fn within_connection_limit<T, E>(
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

    /// The address the guest reads as its own end of a socket holding
    /// `virtual_port`, as cgroup `getsockname4` rewrites it: the guest's own
    /// virtual address and that port, or the unspecified address before a
    /// connect, as Linux answers an unbound socket.
    pub fn local_address(&self, virtual_port: Option<u16>) -> SocketAddr {
        match virtual_port {
            Some(port) => SocketAddr::from((LOCAL_ADDRESS, port)),
            None => SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0)),
        }
    }

    /// The next virtual port, as Linux's `inet_hash_connect` picks a source
    /// port as a connect begins.
    fn alloc_virtual_port(&self) -> u16 {
        let mut next = self
            .next_port
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let port = *next;
        *next = match port == *VIRTUAL_PORTS.end() {
            true => *VIRTUAL_PORTS.start(),
            false => port + 1,
        };
        port
    }

    /// Allocate the virtual address `lookup`'s answer is handed out at, from
    /// the bottom of the range, never reused for a second name; `None` while
    /// the lookup is under way, its failure once it failed, and
    /// `TemporaryFailure` once the range is spent or `cred`'s memory cannot
    /// hold the entry, which the table keeps until the Invocation ends.
    pub fn alloc_virtual_address(
        &self,
        cred: &Cred,
        lookup: &NameLookup,
    ) -> Option<Result<Ipv4Addr, LookupError>> {
        let real = match lookup.answer()? {
            Ok(real) => real,
            Err(err) => return Some(Err(err)),
        };
        let mut virtuals = self.virtuals.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(offset) = u32::try_from(virtuals.len())
            .ok()
            .filter(|o| *o < NAMED_SIZE)
        else {
            return Some(Err(LookupError::TemporaryFailure));
        };
        let entry = (lookup.name().to_string(), real);
        let held = std::mem::size_of_val(&entry)
            + entry.0.len()
            + entry.1.len() * std::mem::size_of::<IpAddr>();
        if cred.security_vm_enough_memory(held).is_err() {
            return Some(Err(LookupError::TemporaryFailure));
        }
        virtuals.push(entry);
        Some(Ok(Ipv4Addr::from(VIRTUAL_BASE + offset)))
    }

    /// The name a virtual address stands for and its real addresses.
    fn translate(&self, ip: Ipv4Addr) -> Option<(String, Vec<IpAddr>)> {
        let offset = u32::from(ip).checked_sub(VIRTUAL_BASE)?;
        self.virtuals
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(usize::try_from(offset).ok()?)
            .cloned()
    }

    /// A TCP socket connecting to `addr`, the address the guest named, as
    /// `cred`. The socket hooks judge what the guest named first, in Linux's
    /// order: the modules `cred` holds, then each entry's policy, any refusal
    /// winning. A literal address then passes the
    /// ruleset as itself. A virtual one is translated to its name's real
    /// addresses, an unmapped one refused as any refused destination is; each
    /// real address the ruleset lets the name lead to is dialed exactly, in
    /// turn, until one answers, so a name answering `::1` first still reaches
    /// `127.0.0.1`.
    pub fn connect(
        &self,
        cred: &Cred,
        sock: &TcpSock,
        addr: SocketAddr,
    ) -> Result<(), FilesystemError> {
        let real = self.admit(cred, addr, Protocol::Tcp)?;
        self.within_connection_limit(FilesystemError::WouldBlock, || {
            sock.connect(real, addr, &self.open_connections)
        })?;
        sock.hold_virtual_port(|| self.alloc_virtual_port());
        Ok(())
    }

    /// Create a TCP socket connected to nothing for `cred`, which connects
    /// through this namespace's Protocol Family, as `sock_create` does; its
    /// buffers count against `cred`'s memory, as a socket's do against its
    /// creator's memory cgroup.
    pub fn create_tcp_socket(&self, cred: &Arc<Cred>) -> Result<Arc<TcpSock>, FilesystemError> {
        TcpSock::new(self.family.clone(), cred)
    }

    /// Create a UDP socket connected to nothing for `cred`, as `sock_create`
    /// does; its receive queue counts against `cred`'s memory.
    pub fn create_udp_socket(&self, cred: &Arc<Cred>) -> Result<Arc<UdpSock>, FilesystemError> {
        UdpSock::new(cred)
    }

    /// Bind a socket to `addr` as `cred`, once the socket hooks pass it, as
    /// `__sys_bind` asks `security_socket_bind`; the host socket takes its
    /// own address as it connects, so a bind the hooks pass leaves nothing
    /// to do here.
    pub fn bind(&self, cred: &Cred, addr: SocketAddr) -> Result<(), FilesystemError> {
        cred.security_socket_bind(addr)?;
        match addr.is_ipv6() {
            true => Err(FilesystemError::AddressFamilyNotSupported),
            false => Ok(()),
        }
    }

    /// Connect a UDP socket to `addr`, the one peer the guest named, as
    /// `cred`. It passes the same steps a TCP connect does, as
    /// `udp_pre_connect` runs `connect4`; UDP opens no handshake, so the first
    /// real address the ruleset admits is its peer. A socket naming no peer
    /// would send each datagram anywhere, so it is refused.
    pub fn connect_datagram(
        &self,
        cred: &Cred,
        sock: &UdpSock,
        addr: Option<SocketAddr>,
    ) -> Result<(), FilesystemError> {
        let addr = addr.ok_or(FilesystemError::PermissionDenied)?;
        let real = self.admit(cred, addr, Protocol::Udp)?;
        self.within_connection_limit(FilesystemError::WouldBlock, || {
            sock.connect(&self.family, real[0], addr, &self.open_connections)
        })?;
        sock.hold_virtual_port(|| self.alloc_virtual_port());
        Ok(())
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

    /// The real addresses a `protocol` connection to `addr` may dial, once the
    /// socket hooks and the ruleset pass it. The hooks judge what the guest
    /// named first; only then does the address meet the guest's family, as
    /// `inet_stream_connect` refuses an IPv6 address after
    /// `security_socket_connect`, and the virtual range's translation, as
    /// cgroup `connect4` runs inside the protocol's `connect`.
    fn admit(
        &self,
        cred: &Cred,
        addr: SocketAddr,
        protocol: Protocol,
    ) -> Result<Vec<SocketAddr>, FilesystemError> {
        let translated = match addr.ip() {
            IpAddr::V4(ip) if u32::from(ip) & !(VIRTUAL_SIZE - 1) == VIRTUAL_BASE => {
                Some(self.translate(ip))
            }
            _ => None,
        };
        let host = match &translated {
            Some(Some((name, _))) => name.clone(),
            _ => addr.ip().to_string(),
        };
        self.judge(cred, &Connection::new(&host, addr.port(), protocol))?;
        if addr.is_ipv6() {
            return Err(FilesystemError::AddressFamilyNotSupported);
        }
        match translated {
            None => {
                self.ruleset.admit_literal(addr, protocol)?;
                Ok(vec![addr])
            }
            Some(None) => Err(FilesystemError::PermissionDenied),
            Some(Some((name, real))) => {
                let local = self
                    .family
                    .local_addresses()
                    .map_err(|_| FilesystemError::PermissionDenied)?;
                let admitted =
                    self.ruleset
                        .admitted_addresses_of(&name, &real, addr.port(), protocol, &local);
                if admitted.is_empty() {
                    return Err(FilesystemError::PermissionDenied);
                }
                Ok(admitted)
            }
        }
    }
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
    type Seen = Arc<Mutex<Vec<(String, u16, Protocol)>>>;

    /// An entry reaching `127.0.0.1:4567` over TCP, its connect policy
    /// recording what it saw and answering `allows`.
    struct Judged {
        allows: bool,
        seen: Seen,
    }

    impl Entry for Judged {
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

    fn namespace(allows: &[bool]) -> (Namespace, Arc<CountsDials>, Seen) {
        within(allows, None, None)
    }

    /// A namespace whose Sandbox holds at most `open` connections at once,
    /// and whose Invocation makes at most `total`.
    fn within(
        allows: &[bool],
        open: Option<usize>,
        total: Option<usize>,
    ) -> (Namespace, Arc<CountsDials>, Seen) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let networks = allows
            .iter()
            .enumerate()
            .map(|(index, allows)| {
                let entry = Judged {
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
    struct Watched {
        reads: AtomicUsize,
        resolved: Arc<AtomicUsize>,
    }

    impl Entry for Watched {
        fn endpoints(&self) -> Vec<Endpoint> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            vec![
                Endpoint::new("::1", 4567).unwrap(),
                Endpoint::new("127.0.0.1", 4567).unwrap(),
            ]
        }

        fn policies(&self) -> Policies {
            let resolved = self.resolved.clone();
            Policies::default()
                .with_connect(Arc::new(|connection: &Connection<'_>| {
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
        let entry = Arc::new(Watched {
            reads: AtomicUsize::new(0),
            resolved: Arc::default(),
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
                ipv6
            ),
            (1, Some(HttpError::Denied), 0, Some(HttpError::Denied)),
            "Namespace::new must read an entry's endpoints once, Namespace::send must ask no \
             resolve policy once a connect hook refused, and must refuse an IPv6 authority an \
             endpoint names"
        );
    }

    // @behavior NW-022
    #[test]
    fn a_connect_past_the_sandboxs_cap_answers_eagain() {
        let (namespace, family, _) = within(&[true], Some(0), None);
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
        let bound = namespace.bind(&cred, "[::]:0".parse().unwrap()).err();
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
    struct NoConnect;

    impl SecurityOperations for NoConnect {
        fn socket_connect(&self, _connection: &Connection<'_>) -> Result<(), FilesystemError> {
            Err(FilesystemError::PermissionDenied)
        }
    }

    // @behavior NW-019
    #[test]
    fn a_security_module_refuses_before_any_entry_policy() {
        let (namespace, family, seen) = namespace(&[true]);
        let cred = prepare_creds_within(&[Arc::new(NoConnect)], None);
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
    struct Resolving {
        answers: Mutex<Vec<IpAddr>>,
        lookups: AtomicUsize,
        dialed: Mutex<Vec<SocketAddr>>,
        local: Vec<IpAddr>,
        local_unreadable: bool,
    }

    impl ProtocolFamily for Resolving {
        fn connect(&self, addr: SocketAddr) -> io::Result<std::net::TcpStream> {
            self.dialed.lock().unwrap().push(addr);
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
    fn resolving(
        endpoints: Vec<Endpoint>,
        refused: &'static str,
        family: Arc<Resolving>,
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
    fn settled(lookup: &NameLookup) -> Result<Vec<IpAddr>, LookupError> {
        loop {
            if let Some(answer) = lookup.answer() {
                return answer;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// The virtual address a lookup of `name` hands out.
    fn looked_up(namespace: &Namespace, name: &str) -> SocketAddr {
        let lookup = namespace.resolve(name).expect("the lookup starts");
        settled(&lookup).expect("the name resolves");
        let ip = namespace
            .alloc_virtual_address(&prepare_creds_within(&[], None), &lookup)
            .expect("the lookup settled")
            .expect("the range has room");
        SocketAddr::new(IpAddr::V4(ip), 6379)
    }

    // @behavior NW-024
    #[test]
    fn a_name_no_endpoint_names_or_a_policy_refuses_answers_nxdomain_unasked() {
        let family = Arc::new(Resolving::default());
        let namespace = resolving(
            vec![tcp("cache.internal", 6379), tcp("*.example.com", 443)],
            "blocked.example.com",
            family.clone(),
        );
        let answers = ["other.internal", "blocked.example.com", "example.com"]
            .map(|name| settled(&namespace.resolve(name).unwrap()));
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
        let family = Arc::new(Resolving {
            answers: Mutex::new(vec!["8.8.8.8".parse().unwrap()]),
            local_unreadable: true,
            ..Resolving::default()
        });
        let namespace = resolving(vec![tcp("api.example.com", 443)], "", family.clone());
        let cred = prepare_creds_within(&[], None);
        let mut at = looked_up(&namespace, "api.example.com");
        at.set_port(443);
        let answer = dial(&namespace, &cred, at).err();
        assert_eq!(
            (answer, family.dialed.lock().unwrap().len()),
            (Some(FilesystemError::PermissionDenied), 0),
            "Namespace::connect must refuse with EACCES, dialing nothing, when the Protocol \
             Family cannot read the host's own addresses, rather than judge the name's \
             addresses as if the host had none"
        );
    }

    // @behavior NW-149
    #[test]
    fn a_virtual_address_is_held_against_the_invocations_memory() {
        let family = Arc::new(Resolving {
            answers: Mutex::new(vec!["127.0.0.1".parse().unwrap()]),
            ..Resolving::default()
        });
        let namespace = resolving(vec![tcp("cache.internal", 6379)], "", family);
        let lookup = namespace
            .resolve("cache.internal")
            .expect("the lookup starts");
        settled(&lookup).expect("the name resolves");
        let full = namespace.alloc_virtual_address(&prepare_creds_within(&[], Some(0)), &lookup);
        let roomy = namespace.alloc_virtual_address(&prepare_creds_within(&[], None), &lookup);
        assert_eq!(
            (full, roomy),
            (
                Some(Err(LookupError::TemporaryFailure)),
                Some(Ok("198.18.0.0".parse().unwrap()))
            ),
            "Namespace::alloc_virtual_address must answer temporary-failure when the \
             Invocation's memory cannot hold the name's entry, and hand out the bottom of the \
             range once it can"
        );
    }

    // @behavior NW-025
    #[test]
    fn a_name_connects_through_its_virtual_address_to_where_its_endpoint_leads() {
        let family = Arc::new(Resolving {
            answers: Mutex::new(vec!["127.0.0.1".parse().unwrap()]),
            ..Resolving::default()
        });
        let local = tcp("cache.internal", 6379).with_addresses(vec![Addresses::Local]);
        let namespace = resolving(vec![local, tcp("api.internal", 6379)], "", family.clone());
        let cred = prepare_creds_within(&[], None);
        let at = looked_up(&namespace, "Cache.Internal.");
        let sock = dial(&namespace, &cred, at).expect("the endpoint leads there");
        while matches!(sock.finish_connect(), Err(FilesystemError::WouldBlock)) {
            std::thread::sleep(Duration::from_millis(1));
        }
        let public_only = looked_up(&namespace, "api.internal");
        assert!(
            at.ip().to_string() == "198.18.0.0"
                && *family.dialed.lock().unwrap() == vec!["127.0.0.1:6379".parse().unwrap()]
                && matches!(
                    dial(&namespace, &cred, public_only),
                    Err(FilesystemError::PermissionDenied)
                ),
            "a connect to a name's virtual address must dial the real address its endpoint's \
             addresses lead to, and refuse one an endpoint leading only to public addresses \
             does not reach"
        );
    }

    // @behavior NW-026
    #[test]
    fn a_virtual_address_no_lookup_handed_out_is_refused() {
        let namespace = resolving(vec![tcp("cache.internal", 6379)], "", Arc::default());
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
        let family = Arc::new(Resolving {
            answers: Mutex::new(vec!["10.0.0.1".parse().unwrap()]),
            ..Resolving::default()
        });
        let private = tcp("cache.internal", 6379).with_addresses(vec![Addresses::Private]);
        let namespace = resolving(vec![private], "", family.clone());
        let at = looked_up(&namespace, "cache.internal");
        *family.answers.lock().unwrap() = vec!["127.0.0.1".parse().unwrap()];
        let cred = prepare_creds_within(&[], None);
        let sock = dial(&namespace, &cred, at).expect("the endpoint leads there");
        while matches!(sock.finish_connect(), Err(FilesystemError::WouldBlock)) {
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(
            (
                family.lookups.load(Ordering::SeqCst),
                family.dialed.lock().unwrap().clone()
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
        let family = Arc::new(Resolving {
            answers: Mutex::new(vec![own]),
            local: vec![own],
            ..Resolving::default()
        });
        let namespace = resolving(vec![tcp("api.example.com", 6379)], "", family.clone());
        let at = looked_up(&namespace, "api.example.com");
        let cred = prepare_creds_within(&[], None);
        assert!(
            matches!(
                dial(&namespace, &cred, at),
                Err(FilesystemError::PermissionDenied)
            ) && family.dialed.lock().unwrap().is_empty(),
            "a name resolving to the host's own public address must be refused by an endpoint \
             leading only to public addresses, since the host's addresses are local"
        );
    }

    // @behavior NW-032
    #[test]
    fn each_address_a_name_may_lead_to_is_dialed_in_turn() {
        let family = Arc::new(Resolving {
            answers: Mutex::new(vec!["::1".parse().unwrap(), "127.0.0.1".parse().unwrap()]),
            ..Resolving::default()
        });
        let local = tcp("localhost", 6379).with_addresses(vec![Addresses::Local]);
        let namespace = resolving(vec![local], "", family.clone());
        let at = looked_up(&namespace, "localhost");
        let cred = prepare_creds_within(&[], None);
        let sock = dial(&namespace, &cred, at).expect("the endpoint leads there");
        while matches!(sock.finish_connect(), Err(FilesystemError::WouldBlock)) {
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(
            *family.dialed.lock().unwrap(),
            vec![
                "[::1]:6379".parse::<SocketAddr>().unwrap(),
                "127.0.0.1:6379".parse().unwrap()
            ],
            "a connect to a name answering ::1 first must dial 127.0.0.1 once ::1 refuses, as \
             each address the endpoint leads to is dialed in turn"
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
