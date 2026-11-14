//! The backends a network namespace reaches the host through, which a Host
//! App may replace, as gVisor picks one network stack for each sandbox.

use std::sync::Arc;

use super::family::{HostInet, ProtocolFamily};
use super::lookup::Resolver;

/// The Protocol Family that dials and the Resolver that answers names, one
/// pair for every entry of an Invocation's network. Both stand beneath the
/// socket layer's rules, so each is handed only what those rules admitted:
/// a family dials a real address the filter let through, and every address
/// a resolver answers is judged again at connect. The L7 proxy's sender is
/// not among them, since the rules hold only while it dials through the
/// connector kobako hands it.
#[derive(Clone)]
pub struct Backends {
    family: Arc<dyn ProtocolFamily>,
    resolver: Arc<dyn Resolver>,
}

impl Backends {
    /// Dial through `family` and look names up through `resolver`.
    pub fn new(family: Arc<dyn ProtocolFamily>, resolver: Arc<dyn Resolver>) -> Self {
        Self { family, resolver }
    }

    pub(crate) fn family(&self) -> &Arc<dyn ProtocolFamily> {
        &self.family
    }

    pub(crate) fn resolver(&self) -> &Arc<dyn Resolver> {
        &self.resolver
    }
}

impl Default for Backends {
    /// The host's own sockets and resolver.
    fn default() -> Self {
        let host = Arc::new(HostInet);
        Self::new(host.clone(), host)
    }
}
