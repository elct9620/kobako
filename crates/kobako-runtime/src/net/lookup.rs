//! A name lookup the guest asked for, judged as a DNS proxy judges a query,
//! then resolved on a thread of its own since a Resolver blocks, its waiter
//! woken through the lookup's wait queue so the deadline ends the wait. Linux
//! leaves resolution to userspace, so the Resolver is a backend beside the
//! Protocol Family.

use std::net::IpAddr;
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Context, Poll, Waker};
use std::thread;

use super::connections::{open, OpenConnections, Place};
use super::Namespace;
use crate::filesystem::anon_inode_getfile;
use crate::network::Name;
use crate::vfs::{Cred, FMode, File, FileOperations, OpenHow, PollMask};
use crate::wait::{poll_wait, PollTable, WaitQueueHead};

/// What answers a name, as userspace's `getaddrinfo` does in Linux, apart
/// from the Protocol Family that dials what it answered. The guest's lookups
/// and the L7 proxy both ask it.
pub trait Resolver: Send + Sync {
    /// The addresses `name`, a fully qualified name, resolves to. A resolver
    /// that resolves nothing answers `Unsupported`.
    fn resolve(&self, name: &str) -> std::io::Result<Vec<IpAddr>>;
}

/// Why a lookup has no address, as WASI's `ip-name-lookup` names each.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum LookupError {
    /// The name resolves to nothing, or no entry lets the guest look it up;
    /// the two answer alike, so a refusal reads as NXDOMAIN.
    NameUnresolvable,
    /// The resolver cannot answer this name at all.
    PermanentFailure,
    /// The lookup cannot start now, as a full cap leaves it.
    TemporaryFailure,
}

/// One lookup in flight, then its answer.
pub struct NameLookup {
    name: String,
    answer: Mutex<Option<Result<Vec<IpAddr>, LookupError>>>,
    wait: Arc<WaitQueueHead>,
    /// The task awaiting the answer on the reactor, if one is.
    task: Mutex<Option<Waker>>,
}

impl NameLookup {
    /// A lookup of `name` through `resolver` on a thread holding one of
    /// `open_connections`' places; a full count answers at once as a temporary
    /// failure.
    pub(crate) fn start(
        resolver: Arc<dyn Resolver>,
        name: String,
        open_connections: &Arc<OpenConnections>,
    ) -> Result<Arc<Self>, LookupError> {
        let place = open(open_connections).ok_or(LookupError::TemporaryFailure)?;
        Self::start_holding(resolver, name, Arc::new(place))
    }

    /// A lookup of `name` through `resolver` on a thread holding `place`,
    /// which its caller already counts. The thread holds the place until the
    /// resolver returns, past the lookup's drop and its caller's, so the
    /// places bound the resolver threads too.
    pub(crate) fn start_holding(
        resolver: Arc<dyn Resolver>,
        name: String,
        place: Arc<Place>,
    ) -> Result<Arc<Self>, LookupError> {
        let lookup = Arc::new(Self {
            name,
            answer: Mutex::new(None),
            wait: Arc::default(),
            task: Mutex::new(None),
        });
        let resolving = lookup.clone();
        // A thread the process cannot make answers as a resolver out of
        // resources does, and gives its place back as the closure drops.
        let spawned = thread::Builder::new()
            .name("kobako-resolver".into())
            .spawn(move || {
                let _place = place;
                let answer = match resolver.resolve(&resolving.name) {
                    Ok(addresses) if addresses.is_empty() => Err(LookupError::NameUnresolvable),
                    Ok(addresses) => Ok(addresses),
                    Err(err) if err.kind() == std::io::ErrorKind::Unsupported => {
                        Err(LookupError::PermanentFailure)
                    }
                    Err(_) => Err(LookupError::NameUnresolvable),
                };
                *resolving
                    .answer
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner) = Some(answer);
                resolving.wait.wake_up();
                if let Some(task) = resolving
                    .task
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .take()
                {
                    task.wake();
                }
            });
        spawned.map_err(|_| LookupError::TemporaryFailure)?;
        Ok(lookup)
    }

    /// A lookup answered at once, with no thread, as a refused name is.
    pub(crate) fn with_answer(name: String, answer: Result<Vec<IpAddr>, LookupError>) -> Arc<Self> {
        Arc::new(Self {
            name,
            answer: Mutex::new(Some(answer)),
            wait: Arc::default(),
            task: Mutex::new(None),
        })
    }

    /// The answer for a task on the reactor, which is woken once it arrives;
    /// the answer is read while the waker is held, so none arrives unseen.
    pub(crate) fn poll_answer(
        &self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Vec<IpAddr>, LookupError>> {
        let mut task = self.task.lock().unwrap_or_else(PoisonError::into_inner);
        match self.answer() {
            Some(answer) => Poll::Ready(answer),
            None => {
                *task = Some(cx.waker().clone());
                Poll::Pending
            }
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

/// The name the guest asked for as the resolver and the hooks judge it:
/// lowercase, with no trailing dot, and an IDN as its A-label. A name no
/// A-label spells has no lookup.
pub(crate) fn normalize(name: &str) -> Result<String, LookupError> {
    crate::network::to_a_label(name).ok_or(LookupError::NameUnresolvable)
}

/// A lookup of `name` in `net`. The name is normalized, then each entry's
/// `resolve` policy and the endpoints judge it, as a DNS proxy judges a
/// query before it forwards one; a refused or unmatched name answers as
/// NXDOMAIN before any query, as a real one would. A name the Invocation
/// already translated answers its first real addresses again, with no query,
/// so a resolver cannot rebind it.
pub fn resolve(net: &Namespace, name: &str) -> Result<Arc<NameLookup>, LookupError> {
    let name = normalize(name)?;
    if !allows_lookup(net, &name) || !net.ruleset().admits_name(&name) {
        return Ok(NameLookup::with_answer(
            name,
            Err(LookupError::NameUnresolvable),
        ));
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
