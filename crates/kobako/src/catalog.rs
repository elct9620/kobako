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

use kobako_codec::codec::{Encoder, Value};

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
        let paths = self
            .bindings
            .iter()
            .map(|(path, _)| Value::Str(path.clone()))
            .collect();
        let mut encoder = Encoder::new();
        encoder
            .write_value(&Value::Array(paths))
            .expect("a str preamble always encodes");
        encoder.into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use kobako_codec::codec::Value;

    use crate::receiver::{Fault, Receiver};

    use super::*;

    struct Probe;

    impl Receiver for Probe {
        fn call(
            &self,
            _method: &str,
            _args: &[Value],
            _kwargs: &[(String, Value)],
            _block: Option<&mut crate::yielder::Yielder<'_>>,
            _handles: &crate::handles::Handles<'_>,
        ) -> Result<Value, Fault> {
            Ok(Value::Nil)
        }
    }

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

    // The preamble byte shape is the guest's registration input; pin
    // the exact encoding for one bound path so drift in the frame
    // builder is caught here rather than inside an E2E run.
    #[test]
    fn preamble_encodes_the_flat_path_list() {
        let mut catalog = Catalog::default();
        catalog.bind("MyService::KV", Arc::new(Probe));
        let expected = {
            let mut encoder = Encoder::new();
            encoder
                .write_value(&Value::Array(vec![Value::Str("MyService::KV".into())]))
                .unwrap();
            encoder.into_bytes()
        };
        assert_eq!(catalog.preamble(), expected);
    }

    #[test]
    fn empty_catalog_preamble_is_the_explicit_empty_array() {
        assert_eq!(Catalog::default().preamble(), vec![0x90]);
    }
}
