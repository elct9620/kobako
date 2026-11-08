//! A name lookup the guest asked for, resolved on a thread of its own since a
//! resolver blocks, its waiter woken through the lookup's wait queue so the deadline
//! ends the wait. Linux leaves resolution to userspace; this resolver is the
//! only one the guest has.

use std::net::IpAddr;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;

use super::connections::{open, OpenConnections};
use super::ProtocolFamily;
use crate::wait::{poll_wait, PollTable, WaitQueueHead};

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
}

impl NameLookup {
    /// A lookup of `name` through `family` on a thread counted against
    /// `workers`; a full count answers at once as a temporary failure.
    pub(crate) fn start(
        family: Arc<dyn ProtocolFamily>,
        name: String,
        open_connections: &Arc<OpenConnections>,
    ) -> Result<Arc<Self>, LookupError> {
        let place = open(open_connections).ok_or(LookupError::TemporaryFailure)?;
        let lookup = Arc::new(Self {
            name,
            answer: Mutex::new(None),
            wait: Arc::default(),
        });
        let resolving = lookup.clone();
        // A thread the process cannot make answers as a resolver out of
        // resources does, and gives its place back as the closure drops.
        let spawned = thread::Builder::new()
            .name("kobako-resolver".into())
            .spawn(move || {
                let _place = place;
                let answer = match family.resolve(&resolving.name) {
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
            });
        spawned.map_err(|_| LookupError::TemporaryFailure)?;
        Ok(lookup)
    }

    /// A lookup answered at once, with no thread, as a refused name is.
    pub(crate) fn answered(name: String, answer: Result<Vec<IpAddr>, LookupError>) -> Arc<Self> {
        Arc::new(Self {
            name,
            answer: Mutex::new(Some(answer)),
            wait: Arc::default(),
        })
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

    /// Whether the lookup has answered, registering on `table` for a poll that
    /// will sleep, as a file's `->poll` does.
    pub fn poll(&self, table: Option<&mut PollTable>) -> bool {
        poll_wait(&self.wait, table);
        self.answer().is_some()
    }
}

/// The name the guest asked for as the resolver and the hooks judge it:
/// lowercase, with no trailing dot, and an IDN as its A-label. A name no
/// A-label spells has no lookup.
pub(crate) fn normalize(name: &str) -> Result<String, LookupError> {
    let name = name.strip_suffix('.').unwrap_or(name);
    match idna::domain_to_ascii(name) {
        Ok(ascii) if !ascii.is_empty() => Ok(ascii),
        _ => Err(LookupError::NameUnresolvable),
    }
}
