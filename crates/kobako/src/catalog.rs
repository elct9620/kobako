//! Per-Sandbox Service registry: the flat path→object bindings, the
//! Frame 1 preamble they encode into, and the preloaded snippet table
//! sealed alongside them.
//!
//! The SDK twin of the Ruby gem's `Kobako::Catalog`: the registration
//! tables fill during setup, seal on the first invocation, and from
//! then on every dispatch and frame read sees one immutable state. The
//! per-invocation capability Handle table lives separately in
//! `crate::handles` — it mutates during dispatch, so it sits outside
//! the sealed state.

use std::sync::Arc;

use kobako_transport::envelope::Preamble;

use crate::receiver::Receiver;
use crate::snippet::Snippets;

/// Bind-ordered Service registry plus the snippet table for one Sandbox.
#[derive(Default)]
pub(crate) struct Catalog {
    bindings: Vec<(String, Arc<dyn Receiver>)>,
    pub(crate) snippets: Snippets,
}

impl Catalog {
    /// Bind a host object as the Service reachable at `path`. Rebinding
    /// an identical path replaces the object — the Ruby frontend refuses
    /// a malformed or colliding path at its own surface, so this registry
    /// stays permissive; a path that is a prefix of another is caught
    /// fail-closed by the guest when it materializes the proxies.
    pub(crate) fn bind(&mut self, path: &str, object: Arc<dyn Receiver>) {
        match self.bindings.iter_mut().find(|(p, _)| p == path) {
            Some((_, slot)) => *slot = object,
            None => self.bindings.push((path.to_string(), object)),
        }
    }

    /// Resolve a dispatch target path to its bound object.
    pub(crate) fn lookup(&self, path: &str) -> Option<Arc<dyn Receiver>> {
        self.bindings
            .iter()
            .find(|(p, _)| p == path)
            .map(|(_, object)| object.clone())
    }

    /// Encode the Frame 1 registration preamble: a flat list of bind
    /// paths (`["MyService::KV", "File"]`) in bind order.
    pub(crate) fn preamble(&self) -> Vec<u8> {
        Preamble {
            paths: self.bindings.iter().map(|(path, _)| path.clone()).collect(),
        }
        .encode()
    }
}

#[cfg(test)]
mod tests {
    use crate::receiver::Probe;

    use super::*;

    #[test]
    fn bind_then_lookup_resolves_the_path() {
        let mut catalog = Catalog::default();
        catalog.bind("MyService::KV", Arc::new(Probe));
        catalog.bind("File", Arc::new(Probe));
        assert!(catalog.lookup("MyService::KV").is_some());
        assert!(catalog.lookup("File").is_some());
        assert!(catalog.lookup("MyService::Other").is_none());
    }

    #[test]
    fn rebind_replaces_the_object_at_the_same_path() {
        let mut catalog = Catalog::default();
        catalog.bind("MyService::KV", Arc::new(Probe));
        catalog.bind("MyService::KV", Arc::new(Probe));
        assert!(catalog.lookup("MyService::KV").is_some());
    }

    // The preamble is the guest's registration input; bind order is the
    // property that matters to it, so read it back through the envelope
    // rather than pinning bytes the envelope's own tests already own.
    #[test]
    fn the_preamble_carries_every_bound_path_in_bind_order() {
        let mut catalog = Catalog::default();
        catalog.bind("MyService::KV", Arc::new(Probe));
        catalog.bind("File", Arc::new(Probe));
        assert_eq!(
            Preamble::decode(&catalog.preamble()),
            Ok(Preamble {
                paths: vec!["MyService::KV".into(), "File".into()]
            }),
            "a bound catalog must send every path on Frame 1 in bind order"
        );
    }

    #[test]
    fn an_empty_catalog_sends_a_present_empty_preamble() {
        assert_eq!(
            Preamble::decode(&Catalog::default().preamble()),
            Ok(Preamble::default()),
            "a catalog with no bindings must send a present, empty Frame 1"
        );
    }
}
