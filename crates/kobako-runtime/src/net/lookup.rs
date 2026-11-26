//! A name lookup the guest asked for, judged as a DNS proxy judges a query,
//! then resolved on the thread that waits for it, woken through the
//! lookup's wait queue so the deadline ends the wait. Linux leaves resolution
//! to userspace, where a process's own query ends with it, so the Resolver is
//! a backend beside the Protocol Family, and dropping a lookup cancels its
//! query.

use std::collections::HashSet;
use std::future::Future;
use std::net::IpAddr;
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Context, Poll};

use super::connections::{open, ConnectionBudget, OpenConnections, Place};
use super::task::Task;
use super::Namespace;
use crate::cred::Cred;
use crate::filesystem::anon_inode_getfile;
use crate::network::{Host, Name};
use crate::vfs::{FMode, File, FileOperations, OpenHow, PollMask};
use crate::wait::{poll_wait, PollTable, WaitQueueHead};

/// glibc's `NS_MAXCDNAME`: the most bytes a name takes on the wire, its
/// length bytes and root label counted.
pub(crate) const NS_MAXCDNAME: usize = 255;

/// A name's addresses still coming, which the asking thread polls as it
/// waits, as reqwest's `Resolving` is.
pub type Resolving = Pin<Box<dyn Future<Output = Result<Vec<IpAddr>, LookupError>> + Send>>;

/// What answers a name, as userspace's `getaddrinfo` does in Linux, apart
/// from the Protocol Family that dials what it answered. The guest's lookups
/// and the L7 proxy both ask it.
pub trait Resolver: Send + Sync {
    /// The addresses `name`, a fully qualified name, resolves to. It is
    /// called on the thread that asks, never the reactor, so starting a query
    /// may wait briefly as a system call does; the future runs on kobako's
    /// reactor, so polling it never blocks, and dropping it cancels the
    /// query, so no query outlives the lookup that asked. A failure names
    /// its kind as an NSS module reports `h_errno` to `getaddrinfo`: no
    /// address answers `NoName` or an empty list, a resolver it cannot reach
    /// now `Again`, and one that cannot answer the name at all `Fail`.
    fn resolve(&self, name: &str) -> Resolving;
}

/// Why a lookup has no address, as `getaddrinfo` names each by its `EAI_`
/// code; a Wasm Driver turns each into its ABI's code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum LookupError {
    /// No URL host spells the name, so it is no domain name or address at
    /// all.
    InvalidName,
    /// `EAI_NONAME`: the name resolves to nothing, or no entry lets the guest
    /// look it up; the two answer alike, so a refusal reads as NXDOMAIN.
    NoName,
    /// `EAI_FAIL`: the resolver cannot answer this name at all.
    Fail,
    /// `EAI_AGAIN`: no answer now, as a full cap, a timeout, or a server
    /// failure leaves it; asking again may answer.
    Again,
}

/// The answer a lookup waits for, shared with the query that sets it.
type Answer = Arc<Mutex<Option<Result<Vec<IpAddr>, LookupError>>>>;

/// One lookup in flight, then its answer.
pub struct NameLookup {
    name: String,
    /// Whether the name spelled an address, which answers as itself.
    of_address: bool,
    answer: Answer,
    wait: Arc<WaitQueueHead>,
    /// The query under way, moved by whoever asks for the answer and
    /// cancelled as the lookup drops.
    query: Option<Task>,
}

impl NameLookup {
    /// A lookup of `name` through `resolver` holding one of
    /// `open_connections`' places and spending one of `budget` before the
    /// query leaves; a full count or a spent budget answers at once as a
    /// temporary failure, asking no resolver.
    pub(crate) fn start(
        resolver: Arc<dyn Resolver>,
        name: String,
        open_connections: &Arc<OpenConnections>,
        budget: &ConnectionBudget,
    ) -> Result<Arc<Self>, LookupError> {
        let place = open(open_connections).ok_or(LookupError::Again)?;
        budget.spend().map_err(|_| LookupError::Again)?;
        Ok(Self::start_holding(resolver, name, Arc::new(place)))
    }

    /// A lookup of `name` through `resolver` holding `place`, which its
    /// caller already counts, its query sent on the asking thread. The query
    /// holds the place until it answers or the lookup drops and cancels it,
    /// so a lookup nobody waits for gives its place back at once.
    pub(crate) fn start_holding(
        resolver: Arc<dyn Resolver>,
        name: String,
        place: Arc<Place>,
    ) -> Arc<Self> {
        let answer = Answer::default();
        let wait = Arc::<WaitQueueHead>::default();
        let resolving = resolver.resolve(&name);
        let (answering, waking) = (answer.clone(), wait.clone());
        let query = Task::new(
            async move {
                let _place = place;
                let found = match resolving.await {
                    Ok(addresses) if addresses.is_empty() => Err(LookupError::NoName),
                    Ok(mut addresses) => {
                        // Each real address once, so each takes one virtual one.
                        let mut seen = HashSet::new();
                        addresses.retain(|ip| seen.insert(*ip));
                        Ok(addresses)
                    }
                    failed => failed,
                };
                *answering.lock().unwrap_or_else(PoisonError::into_inner) = Some(found);
                waking.wake_up();
            },
            wait.clone(),
        );
        query.poll();
        Arc::new(Self {
            name,
            of_address: false,
            answer,
            wait,
            query: Some(query),
        })
    }

    /// A lookup answered at once, with no query, as a refused name is.
    pub(crate) fn with_answer(name: String, answer: Result<Vec<IpAddr>, LookupError>) -> Arc<Self> {
        Arc::new(Self {
            name,
            of_address: false,
            answer: Arc::new(Mutex::new(Some(answer))),
            wait: Arc::default(),
            query: None,
        })
    }

    /// A lookup of a name spelling `ip`, answered as `ip` itself.
    fn from_address(ip: IpAddr) -> Arc<Self> {
        Arc::new(Self {
            name: ip.to_string(),
            of_address: true,
            answer: Arc::new(Mutex::new(Some(Ok(vec![ip])))),
            wait: Arc::default(),
            query: None,
        })
    }

    /// Whether the name spelled an address, answered as itself rather than
    /// at a virtual one.
    pub(crate) fn is_of_address(&self) -> bool {
        self.of_address
    }

    /// The answer for a request waiting on the lookup, which the lookup's
    /// wait queue wakes once it arrives; the answer is read again once the
    /// request is added, so none arrives unseen.
    pub(crate) fn poll_answer(
        &self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Vec<IpAddr>, LookupError>> {
        if let Some(answer) = self.answer() {
            return Poll::Ready(answer);
        }
        self.wait.add(cx.waker());
        match self.answered() {
            Some(answer) => Poll::Ready(answer),
            None => Poll::Pending,
        }
    }

    /// The name looked up, normalized.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The real addresses the name resolved to, or why it has none; `None`
    /// while the lookup is under way. Asking moves the query on.
    pub fn answer(&self) -> Option<Result<Vec<IpAddr>, LookupError>> {
        if let Some(query) = &self.query {
            query.poll();
        }
        self.answered()
    }

    fn answered(&self) -> Option<Result<Vec<IpAddr>, LookupError>> {
        self.answer
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Allocate the lookup's file, opened as `cred`, as `anon_inode_getfile`
    /// does: a poll finds it readable once the lookup answers.
    pub fn alloc_file(self: &Arc<Self>, cred: &Arc<Cred>) -> File {
        let how = OpenHow {
            read: true,
            nonblock: true,
            ..OpenHow::default()
        };
        anon_inode_getfile(Box::new(LookupFile(self.clone())), &how, cred)
    }
}

/// What answers for a lookup's file.
struct LookupFile(Arc<NameLookup>);

impl FileOperations for LookupFile {
    fn poll(&self, _mode: FMode, table: Option<&mut PollTable>) -> PollMask {
        poll_wait(&self.0.wait, table);
        match self.0.answer() {
            Some(_) => PollMask::IN,
            None => PollMask::default(),
        }
    }

    fn is_stream(&self) -> bool {
        true
    }
}

/// A lookup of `name` in `net`. The name is read as a URL host: one spelling
/// an address answers that address, with no query, as WIT returns an address
/// string; an IPv4-mapped one answers its IPv4 address, as WIT never returns a
/// mapped one. Each entry's `resolve` policy and the endpoints judge any
/// other, as a DNS proxy judges a query before it forwards one; a refused or
/// unmatched name answers as NXDOMAIN before any query, as a real one would.
/// A name no DNS query can carry answers NXDOMAIN too, the lookup keeping
/// none of it, as glibc's `res_hnok` refuses it whatever the Resolver.
/// A name the Invocation already translated answers its first real addresses
/// again, with no query, so a resolver cannot rebind it. Only a query spends
/// the connection budget.
pub fn resolve(net: &Namespace, name: &str) -> Result<Arc<NameLookup>, LookupError> {
    let name = match Host::parse(name).ok_or(LookupError::InvalidName)? {
        Host::Address(ip) => return Ok(NameLookup::from_address(ip.to_canonical())),
        Host::Name(name) => name,
    };
    if name.len() + 2 > NS_MAXCDNAME {
        return Ok(NameLookup::with_answer(
            String::new(),
            Err(LookupError::NoName),
        ));
    }
    if !allows_lookup(net, &name) || !net.ruleset().admits_name(&name) {
        return Ok(NameLookup::with_answer(name, Err(LookupError::NoName)));
    }
    let translated = net.real_addresses_of(&name);
    match translated.is_empty() {
        false => Ok(NameLookup::with_answer(name, Ok(translated))),
        true => NameLookup::start(
            net.resolver().clone(),
            name,
            net.open_connections(),
            net.connection_budget(),
        ),
    }
}

/// Whether every `resolve` policy in `net` lets `name`, normalized, be
/// looked up.
pub(crate) fn allows_lookup(net: &Namespace, name: &str) -> bool {
    let request = Name::new(name);
    net.resolve_policies()
        .iter()
        .all(|policy| policy.allows(&request))
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use super::super::connections::open_within;
    use super::super::family::testing::StallsLookups;
    use super::super::reactor::{Reactor, Ready};
    use super::*;

    // @behavior NW-085
    #[test]
    fn a_dropped_lookup_cancels_its_query_and_gives_both_places_back_at_once() {
        let (started, _) = mpsc::channel();
        let resolver = Arc::new(StallsLookups::new(started));
        let invocation = OpenConnections::new(Some(1));
        let process = OpenConnections::new(Some(1));
        let place = open_within(&invocation, &process).expect("both counts have room");
        let lookup =
            NameLookup::start_holding(resolver.clone(), "slow.test".into(), Arc::new(place));
        drop(lookup);
        assert_eq!(
            (
                open_within(&invocation, &process).is_some(),
                resolver.cancelled()
            ),
            (true, 1),
            "a lookup dropped while its resolver has not answered must cancel the resolver's \
             query and give its place back to the invocation's count and the process's at once"
        );
    }

    /// A resolver whose query holds each poll until the test lets it go, as a
    /// slow certificate check holds the thread polling it, telling as it
    /// starts.
    struct TakesLong {
        polling: Mutex<mpsc::Sender<()>>,
        release: Arc<Mutex<mpsc::Receiver<()>>>,
    }

    impl Resolver for TakesLong {
        fn resolve(&self, _name: &str) -> Resolving {
            let polling = self.polling.lock().unwrap().clone();
            let release = self.release.clone();
            Box::pin(std::future::poll_fn(move |_| {
                let _ = polling.send(());
                // The test lets go once it heard the other socket, or drops
                // its end as it fails.
                let _ = release.lock().unwrap().recv();
                Poll::Ready(Ok(Vec::new()))
            }))
        }
    }

    /// A socket that tells of each change.
    struct TellsEvents(Mutex<mpsc::Sender<()>>);

    impl Ready for TellsEvents {
        fn on_event(&self, _events: PollMask) {
            let _ = self.0.lock().unwrap().send(());
        }

        fn abort(&self) {}
    }

    // One sandbox's lookup taking long holds only the thread that waits on
    // it, so another sandbox's socket hears its peer at once.
    // @behavior NW-236
    #[test]
    fn a_slow_lookup_delays_no_other_socket() {
        let (polling, polled) = mpsc::channel();
        let invocation = OpenConnections::new(None);
        let place = open(&invocation).expect("the count has room");
        let (release, released) = mpsc::channel();
        let resolver = Arc::new(TakesLong {
            polling: Mutex::new(polling),
            release: Arc::new(Mutex::new(released)),
        });
        let asking = std::thread::spawn(move || {
            let lookup = NameLookup::start_holding(resolver, "slow.test".into(), Arc::new(place));
            while lookup.answer().is_none() {}
        });
        polled
            .recv_timeout(Duration::from_secs(5))
            .expect("the lookup's query is polled");
        let (tx, rx) = mpsc::channel();
        let ready: Arc<dyn Ready> = Arc::new(TellsEvents(Mutex::new(tx)));
        let addr = "127.0.0.1:0".parse().unwrap();
        let mut socket = mio::net::UdpSocket::bind(addr).expect("a UDP socket binds");
        let _registration = Reactor::current()
            .expect("a reactor starts")
            .register(&mut socket, Arc::downgrade(&ready))
            .expect("the socket registers");
        while rx.try_recv().is_ok() {}
        std::net::UdpSocket::bind(addr)
            .expect("a UDP socket binds")
            .send_to(b"x", socket.local_addr().unwrap())
            .expect("the datagram goes");
        let heard = rx.recv_timeout(crate::sched::testing::MAX_WAIT).is_ok();
        drop(release);
        crate::sched::testing::join("the asking thread", asking);
        assert!(
            heard,
            "a socket whose peer sent must be woken while another Sandbox's lookup is held in \
             its resolver, since only the thread waiting on that lookup polls it"
        );
    }
}
