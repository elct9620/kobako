//! The network one Invocation's entries make, as Linux's `struct net` holds the
//! rules every socket of a namespace passes and the families that dial it. The
//! guest sees only virtual addresses: a lookup hands out one, and a connect to
//! it is translated to the real addresses the name resolved to, as cgroup
//! `connect4` rewrites a destination, so no name is ever resolved twice.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Instant;

use super::lookup::{normalize, LookupError, NameLookup};
use super::{ProtocolFamily, TcpSock, Workers};
use crate::grant::Policy;
use crate::network::{Connection, EntryError, Name, Network, Protocol, Ruleset};
use crate::vfs::{Cred, FilesystemError};

/// The bottom of the virtual range, `198.18.0.0/15`, inside the reserved set,
/// so no real destination is ever in it.
const VIRTUAL_BASE: u32 = u32::from_be_bytes([198, 18, 0, 0]);

/// How many virtual addresses the range holds.
const VIRTUAL_SIZE: u32 = 1 << 17;

/// What the guest may reach, which policies judge each lookup and connect, the
/// backend that dials it, the Sandbox's workers each socket counts against, and
/// the virtual addresses handed out.
pub struct Namespace {
    ruleset: Ruleset,
    resolve: Vec<Arc<dyn for<'a> Policy<Name<'a>>>>,
    connect: Vec<Arc<dyn for<'a> Policy<Connection<'a>>>>,
    family: Arc<dyn ProtocolFamily>,
    workers: Arc<Workers>,
    /// Each name looked up and its real addresses, at its virtual address's
    /// offset from the bottom of the range.
    virtuals: Mutex<Vec<(String, Vec<IpAddr>)>>,
}

impl Namespace {
    /// The namespace every entry of `networks` makes together, dialing through
    /// `family` on workers counted against `workers`, refused for an endpoint
    /// the ruleset cannot compile.
    pub fn new<'a>(
        networks: impl IntoIterator<Item = &'a Network>,
        family: Arc<dyn ProtocolFamily>,
        workers: Arc<Workers>,
    ) -> Result<Self, EntryError> {
        let networks = networks.into_iter().collect::<Vec<_>>();
        let ruleset = Ruleset::new(networks.iter().flat_map(|network| network.endpoints()))?;
        let policies = networks
            .iter()
            .map(|network| network.policies())
            .collect::<Vec<_>>();
        Ok(Self {
            ruleset,
            resolve: policies.iter().filter_map(|p| p.resolve.clone()).collect(),
            connect: policies.iter().filter_map(|p| p.connect.clone()).collect(),
            family,
            workers,
            virtuals: Mutex::new(Vec::new()),
        })
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
            return Ok(NameLookup::answered(
                name,
                Err(LookupError::NameUnresolvable),
            ));
        }
        NameLookup::start(self.family.clone(), name, &self.workers)
    }

    /// The virtual address `lookup`'s answer is handed out at, allocated from
    /// the bottom of the range and never reused for a second name; `None`
    /// once the range is spent.
    pub fn virtual_address(&self, name: &str, real: Vec<IpAddr>) -> Option<Ipv4Addr> {
        let mut virtuals = self.virtuals.lock().unwrap_or_else(PoisonError::into_inner);
        let offset = u32::try_from(virtuals.len())
            .ok()
            .filter(|o| *o < VIRTUAL_SIZE)?;
        virtuals.push((name.to_string(), real));
        Some(Ipv4Addr::from(VIRTUAL_BASE + offset))
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
    /// `cred`, giving up at `deadline`. The socket hooks judge what the guest
    /// named first, in Linux's order: the modules `cred` holds, then each
    /// entry's policy, any refusal winning. A literal address then passes the
    /// ruleset as itself. A virtual one is translated to its name's real
    /// addresses, an unmapped one answering `ENETUNREACH`, and the first real
    /// address the ruleset lets the name lead to is dialed exactly.
    pub fn connect(
        &self,
        cred: &Cred,
        addr: SocketAddr,
        deadline: Option<Instant>,
    ) -> Result<Arc<TcpSock>, FilesystemError> {
        let translated = match addr.ip() {
            IpAddr::V4(ip) if u32::from(ip) & !(VIRTUAL_SIZE - 1) == VIRTUAL_BASE => Some(
                self.translate(ip)
                    .ok_or(FilesystemError::NetworkUnreachable)?,
            ),
            _ => None,
        };
        let host = match &translated {
            Some((name, _)) => name.clone(),
            None => addr.ip().to_string(),
        };
        let connection = Connection::new(&host, addr.port(), Protocol::Tcp);
        cred.security_socket_connect(&connection)?;
        if !self.connect.iter().all(|policy| policy.allows(&connection)) {
            return Err(FilesystemError::PermissionDenied);
        }
        let real = match translated {
            None => {
                self.ruleset.admit_literal(addr, Protocol::Tcp)?;
                addr
            }
            Some((name, real)) => {
                let local = self.family.local_addresses();
                real.into_iter()
                    .map(|ip| SocketAddr::new(ip, addr.port()))
                    .find(|real| {
                        self.ruleset
                            .admit_resolved(&name, *real, Protocol::Tcp, &local)
                            .is_ok()
                    })
                    .ok_or(FilesystemError::PermissionDenied)?
            }
        };
        TcpSock::connect(self.family.clone(), real, deadline, &self.workers)
    }
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;
    use std::time::Duration;

    use super::*;
    use crate::net::{Stream, SANDBOX_WORKERS};
    use crate::network::{Addresses, Endpoint, Entry, Policies};
    use crate::security::prepare_creds_within;
    use crate::vfs::SecurityOperations;

    /// A family refusing every dial, counting how often it is asked.
    #[derive(Default)]
    struct Counted(AtomicUsize);

    impl ProtocolFamily for Counted {
        fn connect(
            &self,
            _addr: SocketAddr,
            _timeout: Option<Duration>,
        ) -> io::Result<Box<dyn Stream>> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Err(io::ErrorKind::ConnectionRefused.into())
        }
    }

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

    fn namespace(allows: &[bool]) -> (Namespace, Arc<Counted>, Seen) {
        within_workers(allows, SANDBOX_WORKERS)
    }

    /// A namespace whose Sandbox runs at most `workers` workers.
    fn within_workers(allows: &[bool], workers: usize) -> (Namespace, Arc<Counted>, Seen) {
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
        let family = Arc::new(Counted::default());
        let namespace = Namespace::new(&networks, family.clone(), Workers::new(workers))
            .expect("the endpoints compile");
        (namespace, family, seen)
    }

    // @behavior NW-022
    #[test]
    fn a_connect_past_the_sandboxs_workers_answers_eagain() {
        let (namespace, family, _) = within_workers(&[true], 1);
        let cred = prepare_creds_within(&[], None);
        let answer = namespace.connect(&cred, "127.0.0.1:4567".parse().unwrap(), None);
        assert!(
            matches!(answer, Err(FilesystemError::WouldBlock))
                && family.0.load(Ordering::SeqCst) == 0,
            "Namespace::connect whose socket's workers exceed the Sandbox's cap must answer \
             EAGAIN, dialing nothing"
        );
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
        let answer = namespace.connect(&cred, "127.0.0.1:4567".parse().unwrap(), None);
        assert!(
            matches!(answer, Err(FilesystemError::PermissionDenied))
                && family.0.load(Ordering::SeqCst) == 0,
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
        let sock = namespace
            .connect(&cred, "127.0.0.1:4567".parse().unwrap(), None)
            .expect("every policy allows");
        settle(&sock);
        assert_eq!(
            family.0.load(Ordering::SeqCst),
            1,
            "Namespace::connect every connect policy allows must dial the address once"
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
        let answer = namespace.connect(&cred, "127.0.0.1:4567".parse().unwrap(), None);
        assert!(
            matches!(answer, Err(FilesystemError::PermissionDenied))
                && seen.lock().unwrap().is_empty()
                && family.0.load(Ordering::SeqCst) == 0,
            "a Wasm Driver's security module refusing socket_connect must refuse before any \
             entry's connect policy is asked, dialing nothing"
        );
    }

    /// A family resolving every name to what `answers` holds, counting each
    /// lookup, recording each address dialed, and holding `local` as the
    /// host's own addresses.
    #[derive(Default)]
    struct Resolving {
        answers: Mutex<Vec<IpAddr>>,
        lookups: AtomicUsize,
        dialed: Mutex<Vec<SocketAddr>>,
        local: Vec<IpAddr>,
    }

    impl ProtocolFamily for Resolving {
        fn connect(
            &self,
            addr: SocketAddr,
            _timeout: Option<Duration>,
        ) -> io::Result<Box<dyn Stream>> {
            self.dialed.lock().unwrap().push(addr);
            Err(io::ErrorKind::ConnectionRefused.into())
        }

        fn resolve(&self, _name: &str) -> io::Result<Vec<IpAddr>> {
            self.lookups.fetch_add(1, Ordering::SeqCst);
            Ok(self.answers.lock().unwrap().clone())
        }

        fn local_addresses(&self) -> Vec<IpAddr> {
            self.local.clone()
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
        Namespace::new([&network], family, Workers::new(SANDBOX_WORKERS))
            .expect("the endpoints compile")
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
        let real = settled(&lookup).expect("the name resolves");
        let ip = namespace
            .virtual_address(lookup.name(), real)
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
        let sock = namespace
            .connect(&cred, at, None)
            .expect("the endpoint leads there");
        while matches!(sock.finish_connect(), Err(FilesystemError::WouldBlock)) {
            std::thread::sleep(Duration::from_millis(1));
        }
        let public_only = looked_up(&namespace, "api.internal");
        assert!(
            at.ip().to_string() == "198.18.0.0"
                && *family.dialed.lock().unwrap() == vec!["127.0.0.1:6379".parse().unwrap()]
                && matches!(
                    namespace.connect(&cred, public_only, None),
                    Err(FilesystemError::PermissionDenied)
                ),
            "a connect to a name's virtual address must dial the real address its endpoint's \
             addresses lead to, and refuse one an endpoint leading only to public addresses \
             does not reach"
        );
    }

    // @behavior NW-026
    #[test]
    fn a_virtual_address_no_lookup_handed_out_answers_enetunreach() {
        let namespace = resolving(vec![tcp("cache.internal", 6379)], "", Arc::default());
        let cred = prepare_creds_within(&[], None);
        assert!(
            matches!(
                namespace.connect(&cred, "198.18.0.9:6379".parse().unwrap(), None),
                Err(FilesystemError::NetworkUnreachable)
            ),
            "a connect to a virtual address no lookup handed out must answer ENETUNREACH"
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
        let sock = namespace
            .connect(&cred, at, None)
            .expect("the endpoint leads there");
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
                namespace.connect(&cred, at, None),
                Err(FilesystemError::PermissionDenied)
            ) && family.dialed.lock().unwrap().is_empty(),
            "a name resolving to the host's own public address must be refused by an endpoint \
             leading only to public addresses, since the host's addresses are local"
        );
    }
}
