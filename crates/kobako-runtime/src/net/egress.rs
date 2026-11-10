//! The HTTP layer's own judgment, as an egress proxy makes it beside the
//! kernel's: each entry's request policies, and the credentials bound to its
//! own endpoints. The namespace judges reach first, at layer 4.

use std::sync::Arc;

use super::credential::{carries, Swap};
use crate::grant::Policy;
use crate::network::{Credential, Request, Ruleset};

/// What judges a request the namespace let reach its authority, and swaps a
/// credential into it.
pub(crate) struct EgressProxy {
    pub(crate) request: Vec<Arc<dyn for<'a> Policy<Request<'a>>>>,
    /// Each entry's own endpoints and the credentials bound to them.
    pub(crate) bound: Vec<(Ruleset, Vec<Credential>)>,
}

impl EgressProxy {
    /// Whether every request policy lets `sent` leave.
    pub(crate) fn admits(&self, sent: &Request<'_>) -> bool {
        self.request.iter().all(|policy| policy.allows(sent))
    }

    /// The credentials bound to `sent`, a request over `https`: an entry's own
    /// endpoint names its exact host and allows it, and the placement's header
    /// carries the placeholder. Each source answers here, on the guest thread,
    /// so its time counts against the deadline.
    pub(crate) fn swaps_of(&self, sent: &Request<'_>, headers: &[(String, Vec<u8>)]) -> Vec<Swap> {
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
}
