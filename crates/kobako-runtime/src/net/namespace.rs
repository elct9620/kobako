//! The network one Invocation's entries make, as Linux's `struct net` holds the
//! rules every socket of a namespace passes and the families that dial it.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;

use super::{ProtocolFamily, TcpSock};
use crate::grant::Policy;
use crate::network::{Connection, EntryError, Network, Protocol, Ruleset};
use crate::vfs::{Cred, FilesystemError};

/// What the guest may reach, which policies judge each connect, and the backend
/// that dials it.
pub struct Namespace {
    ruleset: Ruleset,
    connect: Vec<Arc<dyn for<'a> Policy<Connection<'a>>>>,
    family: Arc<dyn ProtocolFamily>,
}

impl Namespace {
    /// The namespace every entry of `networks` makes together, dialing through
    /// `family`, refused for an endpoint the ruleset cannot compile.
    pub fn new<'a>(
        networks: impl IntoIterator<Item = &'a Network>,
        family: Arc<dyn ProtocolFamily>,
    ) -> Result<Self, EntryError> {
        let networks = networks.into_iter().collect::<Vec<_>>();
        let ruleset = Ruleset::new(networks.iter().flat_map(|network| network.endpoints()))?;
        let connect = networks
            .iter()
            .filter_map(|network| network.policies().connect)
            .collect();
        Ok(Self {
            ruleset,
            connect,
            family,
        })
    }

    /// A TCP socket connecting to `addr`, the literal address the guest named,
    /// as `cred`, giving up at `deadline`. The socket hooks judge it first, in
    /// Linux's order: the modules `cred` holds, then each entry's policy, any
    /// refusal winning; then the ruleset judges the address. Only an address
    /// that passed is dialed.
    pub fn connect(
        &self,
        cred: &Cred,
        addr: SocketAddr,
        deadline: Option<Instant>,
    ) -> Result<Arc<TcpSock>, FilesystemError> {
        let host = addr.ip().to_string();
        let connection = Connection::new(&host, addr.port(), Protocol::Tcp);
        cred.security_socket_connect(&connection)?;
        if !self.connect.iter().all(|policy| policy.allows(&connection)) {
            return Err(FilesystemError::PermissionDenied);
        }
        self.ruleset.admit_literal(addr, Protocol::Tcp)?;
        Ok(TcpSock::connect(self.family.clone(), addr, deadline))
    }
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;
    use std::time::Duration;

    use super::*;
    use crate::net::Stream;
    use crate::network::{Endpoint, Entry, Policies};
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
        let namespace = Namespace::new(&networks, family.clone()).expect("the endpoints compile");
        (namespace, family, seen)
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
}
