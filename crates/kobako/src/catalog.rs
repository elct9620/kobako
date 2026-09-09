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

use kobako_transport::envelope::Bindings;

use crate::error::Error;
use crate::receiver::Receiver;
use crate::snippet::Snippets;

/// Whether `path` is one or more constant-form segments joined by `::`.
fn is_constant_path(path: &str) -> bool {
    !path.is_empty()
        && path.split("::").all(|segment| {
            let mut chars = segment.chars();
            chars.next().is_some_and(|c| c.is_ascii_uppercase())
                && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        })
}

/// Bind-ordered Service registry plus the snippet table for one Sandbox.
#[derive(Default)]
pub(crate) struct Catalog {
    bindings: Vec<(String, Arc<dyn Receiver>)>,
    pub(crate) snippets: Snippets,
}

impl Catalog {
    /// Bind a host object as the Service reachable at `path`. A path is
    /// constant-form segments joined by `::`, and one already bound —
    /// or standing as another's namespace — is refused rather than
    /// replaced: the guest reaches a Service by that name alone, so a
    /// name meaning two things is a name the guest cannot resolve.
    pub(crate) fn bind(&mut self, path: &str, object: Arc<dyn Receiver>) -> Result<(), Error> {
        if !is_constant_path(path) {
            return Err(Error::Argument(format!(
                "bind path must be constant-form segments joined by '::' (got {path:?})"
            )));
        }
        if self.collides(path) {
            return Err(Error::Argument(format!(
                "Service path {path} conflicts with an existing binding"
            )));
        }
        self.bindings.push((path.to_string(), object));
        Ok(())
    }

    /// Whether `path` names, contains, or sits inside an existing binding.
    fn collides(&self, path: &str) -> bool {
        self.bindings.iter().any(|(existing, _)| {
            existing == path
                || existing.starts_with(&format!("{path}::"))
                || path.starts_with(&format!("{existing}::"))
        })
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
        Bindings {
            paths: self.bindings.iter().map(|(path, _)| path.clone()).collect(),
        }
        .encode()
    }
}

#[cfg(test)]
mod tests {
    use crate::receiver::Probe;

    use super::*;

    fn bound(catalog: &mut Catalog, path: &str) -> Result<(), Error> {
        catalog.bind(path, Arc::new(Probe))
    }

    // @behavior SV-001
    #[test]
    fn bind_then_lookup_resolves_the_path() {
        let mut catalog = Catalog::default();
        bound(&mut catalog, "MyService::KV").expect("a constant path binds");
        bound(&mut catalog, "File").expect("a single segment is a whole path");
        assert!(catalog.lookup("MyService::KV").is_some());
        assert!(catalog.lookup("File").is_some());
        assert!(catalog.lookup("MyService::Other").is_none());
    }

    // @behavior SV-012
    #[test]
    fn a_path_already_bound_is_refused_rather_than_replaced() {
        let mut catalog = Catalog::default();
        bound(&mut catalog, "MyService::KV").expect("the first bind stands");
        assert!(
            bound(&mut catalog, "MyService::KV").is_err(),
            "a path already bound must be refused, since the guest reaches a Service by \
             that name alone"
        );
    }

    // @behavior SV-013
    #[test]
    fn a_path_extending_a_bound_one_is_refused() {
        let mut catalog = Catalog::default();
        bound(&mut catalog, "File").expect("the first bind stands");
        assert!(
            bound(&mut catalog, "File::Reader").is_err(),
            "a name cannot be a Service and a namespace at once"
        );
    }

    // @behavior SV-014
    #[test]
    fn a_path_that_is_a_bound_ones_namespace_is_refused_too() {
        let mut catalog = Catalog::default();
        bound(&mut catalog, "File::Reader").expect("the first bind stands");
        assert!(
            bound(&mut catalog, "File").is_err(),
            "a grouping cannot become a Service either"
        );
    }

    // @behavior SV-030
    #[test]
    fn a_segment_that_is_not_a_constant_name_is_refused() {
        for path in ["lower", "1X", "", "Na-me", "My::lower", "My::"] {
            let mut catalog = Catalog::default();
            assert!(
                bound(&mut catalog, path).is_err(),
                "{path:?} is not constant-form segments joined by '::' and must be refused"
            );
        }
    }

    // The preamble is the guest's registration input; bind order is the
    // property that matters to it, so read it back through the envelope
    // rather than pinning bytes the envelope's own tests already own.
    // @behavior S-020
    #[test]
    fn the_preamble_carries_every_bound_path_in_bind_order() {
        let mut catalog = Catalog::default();
        bound(&mut catalog, "MyService::KV").expect("a constant path binds");
        bound(&mut catalog, "File").expect("a single segment is a whole path");
        assert_eq!(
            Bindings::decode(&catalog.preamble()),
            Ok(Bindings {
                paths: vec!["MyService::KV".into(), "File".into()]
            }),
            "a bound catalog must send every path on Frame 1 in bind order"
        );
    }

    // @behavior WE-050
    #[test]
    fn an_empty_catalog_sends_a_present_empty_preamble() {
        assert_eq!(
            Bindings::decode(&Catalog::default().preamble()),
            Ok(Bindings::default()),
            "a catalog with no bindings must send a present, empty Frame 1"
        );
    }
}
