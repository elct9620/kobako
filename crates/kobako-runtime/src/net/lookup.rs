//! A name lookup the guest asked for, judged as a DNS proxy judges a query,
//! then resolved as a task on the reactor, its waiter woken through the
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

use super::connections::{open, OpenConnections, Place};
use super::reactor::{Reactor, Task};
use super::Namespace;
use crate::cred::Cred;
use crate::filesystem::anon_inode_getfile;
use crate::network::{Host, Name};
use crate::vfs::{FMode, File, FileOperations, OpenHow, PollMask};
use crate::wait::{poll_wait, PollTable, WaitQueueHead};

/// A name's addresses still coming, which the reactor polls, as reqwest's
/// `Resolving` is.
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
    /// The query under way on the reactor, cancelled as the lookup drops.
    query: Option<Arc<Task>>,
}

impl NameLookup {
    /// A lookup of `name` through `resolver` holding one of
    /// `open_connections`' places; a full count answers at once as a temporary
    /// failure.
    pub(crate) fn start(
        resolver: Arc<dyn Resolver>,
        name: String,
        open_connections: &Arc<OpenConnections>,
    ) -> Result<Arc<Self>, LookupError> {
        let place = open(open_connections).ok_or(LookupError::Again)?;
        Self::start_holding(resolver, name, Arc::new(place))
    }

    /// A lookup of `name` through `resolver` holding `place`, which its
    /// caller already counts. The query holds the place until it answers or
    /// the lookup drops and cancels it, so a lookup nobody waits for gives its
    /// place back at once. A reactor the process cannot start answers as a
    /// resolver out of resources does.
    pub(crate) fn start_holding(
        resolver: Arc<dyn Resolver>,
        name: String,
        place: Arc<Place>,
    ) -> Result<Arc<Self>, LookupError> {
        let reactor = Reactor::current().map_err(|_| LookupError::Again)?;
        let answer = Answer::default();
        let wait = Arc::<WaitQueueHead>::default();
        let resolving = resolver.resolve(&name);
        let (answering, waking) = (answer.clone(), wait.clone());
        let query = reactor.spawn(async move {
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
        });
        Ok(Arc::new(Self {
            name,
            of_address: false,
            answer,
            wait,
            query: Some(query),
        }))
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

    /// The answer for a task on the reactor, which the lookup's wait queue
    /// wakes once it arrives; the answer is read again once the task is
    /// added, so none arrives unseen.
    pub(crate) fn poll_answer(
        &self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Vec<IpAddr>, LookupError>> {
        if let Some(answer) = self.answer() {
            return Poll::Ready(answer);
        }
        self.wait.add(cx.waker());
        match self.answer() {
            Some(answer) => Poll::Ready(answer),
            None => Poll::Pending,
        }
    }

    /// The name looked up, normalized.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The real addresses the name resolved to, or why it has none; `None`
    /// while the lookup is under way.
    pub fn answer(&self) -> Option<Result<Vec<IpAddr>, LookupError>> {
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

impl Drop for NameLookup {
    fn drop(&mut self) {
        if let Some(query) = &self.query {
            query.cancel();
        }
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
/// A name the Invocation already translated answers its first real addresses
/// again, with no query, so a resolver cannot rebind it.
pub fn resolve(net: &Namespace, name: &str) -> Result<Arc<NameLookup>, LookupError> {
    let name = match Host::parse(name).ok_or(LookupError::InvalidName)? {
        Host::Address(ip) => return Ok(NameLookup::from_address(ip.to_canonical())),
        Host::Name(name) => name,
    };
    if !allows_lookup(net, &name) || !net.ruleset().admits_name(&name) {
        return Ok(NameLookup::with_answer(name, Err(LookupError::NoName)));
    }
    let translated = net.real_addresses_of(&name);
    match translated.is_empty() {
        false => Ok(NameLookup::with_answer(name, Ok(translated))),
        true => NameLookup::start(net.resolver().clone(), name, net.open_connections()),
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

    use super::super::connections::open_within;
    use super::super::family::testing::StallsLookups;
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
            NameLookup::start_holding(resolver.clone(), "slow.test".into(), Arc::new(place))
                .expect("the lookup starts");
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
}
