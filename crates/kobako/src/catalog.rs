//! Per-Sandbox Service registry: Namespaces, bound Members, the
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

use crate::member::Member;
use crate::snippet::Snippets;

/// Registration-ordered Service registry plus the snippet table for
/// one Sandbox.
#[derive(Default)]
pub(crate) struct Catalog {
    namespaces: Vec<Namespace>,
    pub(crate) snippets: Snippets,
}

struct Namespace {
    name: String,
    members: Vec<(String, Arc<dyn Member>)>,
}

impl Catalog {
    /// Declare a Namespace; declaring the same name again is a no-op,
    /// like the Ruby `#define`.
    pub(crate) fn define(&mut self, namespace: &str) {
        if self.position(namespace).is_none() {
            self.namespaces.push(Namespace {
                name: namespace.to_string(),
                members: Vec::new(),
            });
        }
    }

    /// Bind a Member under `namespace`, declaring the Namespace when
    /// absent. Rebinding an existing name replaces the object — the
    /// Ruby frontend refuses this at its own surface, so the registry
    /// itself stays permissive.
    pub(crate) fn bind(&mut self, namespace: &str, member: &str, object: Arc<dyn Member>) {
        self.define(namespace);
        let ns = self
            .position(namespace)
            .map(|i| &mut self.namespaces[i])
            .expect("define above guarantees the namespace exists");
        match ns.members.iter_mut().find(|(name, _)| name == member) {
            Some((_, slot)) => *slot = object,
            None => ns.members.push((member.to_string(), object)),
        }
    }

    /// Resolve a dispatch target path (`"<Namespace>::<Member>"`) to
    /// its bound object.
    pub(crate) fn lookup(&self, path: &str) -> Option<Arc<dyn Member>> {
        let (namespace, member) = path.split_once("::")?;
        let ns = &self.namespaces[self.position(namespace)?];
        ns.members
            .iter()
            .find(|(name, _)| name == member)
            .map(|(_, object)| object.clone())
    }

    /// Encode the Frame 1 registration preamble:
    /// `[["Namespace", ["Member", ...]], ...]` in registration order.
    pub(crate) fn preamble(&self) -> Vec<u8> {
        let groups = self
            .namespaces
            .iter()
            .map(|ns| {
                let members = ns
                    .members
                    .iter()
                    .map(|(name, _)| Value::Str(name.clone()))
                    .collect();
                Value::Array(vec![Value::Str(ns.name.clone()), Value::Array(members)])
            })
            .collect();
        let mut encoder = Encoder::new();
        encoder
            .write_value(&Value::Array(groups))
            .expect("a str/array preamble always encodes");
        encoder.into_bytes()
    }

    fn position(&self, namespace: &str) -> Option<usize> {
        self.namespaces.iter().position(|ns| ns.name == namespace)
    }
}

#[cfg(test)]
mod tests {
    use kobako_codec::codec::Value;

    use crate::member::{Fault, Member};

    use super::*;

    struct Probe;

    impl Member for Probe {
        fn call(
            &self,
            _method: &str,
            _args: &[Value],
            _kwargs: &[(String, Value)],
            _block: Option<&mut crate::block::Block<'_>>,
            _handles: &crate::handles::Handles<'_>,
        ) -> Result<Value, Fault> {
            Ok(Value::Nil)
        }
    }

    #[test]
    fn bind_then_lookup_resolves_the_two_level_path() {
        let mut catalog = Catalog::default();
        catalog.bind("MyService", "KV", Arc::new(Probe));
        assert!(catalog.lookup("MyService::KV").is_some());
        assert!(catalog.lookup("MyService::Other").is_none());
        assert!(catalog.lookup("Elsewhere::KV").is_none());
        assert!(catalog.lookup("NoSeparator").is_none());
    }

    #[test]
    fn define_is_idempotent() {
        let mut catalog = Catalog::default();
        catalog.define("MyService");
        catalog.bind("MyService", "KV", Arc::new(Probe));
        catalog.define("MyService");
        assert!(catalog.lookup("MyService::KV").is_some());
    }

    // The preamble byte shape is the guest's registration input; pin
    // the exact encoding for one namespace with one member so drift in
    // the frame builder is caught here rather than inside an E2E run.
    #[test]
    fn preamble_encodes_registration_groups() {
        let mut catalog = Catalog::default();
        catalog.bind("MyService", "KV", Arc::new(Probe));
        let expected = {
            let mut encoder = Encoder::new();
            encoder
                .write_value(&Value::Array(vec![Value::Array(vec![
                    Value::Str("MyService".into()),
                    Value::Array(vec![Value::Str("KV".into())]),
                ])]))
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
