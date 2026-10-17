//! Per-Sandbox Service registry: the flat path→object bindings, the
//! paths the guest installs them from, and the preloaded snippet, flag,
//! and Grant tables sealed alongside them.
//!
//! The SDK twin of the Ruby gem's `Kobako::Catalog`: the registration
//! tables fill during setup, seal on the first invocation, and from
//! then on every dispatch and Preamble sees one immutable state. The
//! per-invocation capability Handle table lives separately in
//! `crate::handles` — it mutates during dispatch, so it sits outside
//! the sealed state.

use std::sync::Arc;

use kobako_runtime::runtime::Runtime;
use kobako_transport::envelope::{Bindings, Flag, Preamble};

use crate::error::Error;
use crate::grants::Grants;
use crate::receiver::Receiver;
use crate::snippet::{constant_name, Snippets};

/// The Guest Binary keeps this namespace for constants it may add later.
const RESERVED_NAMESPACE: &str = "Kobako";

/// An empty path splits into one empty segment, which no constant name is.
fn is_constant_path(path: &str) -> bool {
    path.split("::").all(constant_name)
}

fn is_reserved(path: &str) -> bool {
    path.split("::").next() == Some(RESERVED_NAMESPACE)
}

/// Bind-ordered Service registry plus the snippet and flag tables for one
/// Sandbox.
#[derive(Default)]
pub(crate) struct Catalog {
    bindings: Vec<(String, Arc<dyn Receiver>)>,
    pub(crate) snippets: Snippets,
    flags: Vec<Flag>,
    pub(crate) grants: Grants,
}

impl Catalog {
    /// A path already bound, or standing as another's namespace, is refused
    /// rather than replaced: the guest reaches a Service by that name alone,
    /// so a name meaning two things is one it cannot resolve.
    pub(crate) fn bind(&mut self, path: &str, object: Arc<dyn Receiver>) -> Result<(), Error> {
        if !is_constant_path(path) {
            return Err(Error::Argument(format!(
                "bind path must be constant-form segments joined by '::' (got {path:?})"
            )));
        }
        if is_reserved(path) {
            return Err(Error::Argument(format!(
                "Service path {path} is in the reserved {RESERVED_NAMESPACE} namespace"
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

    fn collides(&self, path: &str) -> bool {
        self.bindings.iter().any(|(existing, _)| {
            existing == path
                || existing.starts_with(&format!("{path}::"))
                || path.starts_with(&format!("{existing}::"))
        })
    }

    pub(crate) fn lookup(&self, path: &str) -> Option<Arc<dyn Receiver>> {
        self.bindings
            .iter()
            .find(|(p, _)| p == path)
            .map(|(_, object)| object.clone())
    }

    /// A name set twice is refused, so one flag never carries two values.
    pub(crate) fn flag(&mut self, name: &str, value: &str, critical: bool) -> Result<(), Error> {
        if self.flags.iter().any(|flag| flag.name == name) {
            return Err(Error::Argument(format!("flag {name:?} already set")));
        }
        self.flags.push(Flag {
            name: name.to_string(),
            value: value.to_string(),
            critical,
        });
        Ok(())
    }

    /// The Preamble every guest entry reads, so an invocation and a compile
    /// cannot drift in what they send.
    pub(crate) fn preamble(&self) -> Preamble {
        let mut preamble = Preamble::default();
        preamble.bindings = Bindings {
            paths: self.bindings.iter().map(|(path, _)| path.clone()).collect(),
        };
        preamble.snippets = self.snippets.preloaded().clone();
        preamble.flags = self.flags.clone();
        preamble
    }

    /// Compiles the source snippets under the Preamble an invocation sends,
    /// so the replayed bytecode saw the same bindings and flags.
    pub(crate) fn compile(&mut self, runtime: &dyn Runtime) {
        let preamble = self.preamble();
        self.snippets.compile(runtime, &preamble);
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

    // mruby holds a symbol of at most 65534 bytes, so a segment one byte
    // longer could never be interned as the constant it names.
    // @behavior SV-049
    #[test]
    fn a_segment_past_the_symbol_limit_is_refused() {
        let path = format!("My::A{}", "b".repeat(65_534));
        assert!(
            bound(&mut Catalog::default(), &path).is_err(),
            "a path whose segment is one byte past the symbol limit through Catalog::bind must be refused"
        );
    }

    // A segment merely starting with the reserved name is another segment,
    // so it stays bindable.
    // @behavior SV-050
    #[test]
    fn a_path_in_the_reserved_namespace_is_refused() {
        let mut catalog = Catalog::default();
        for path in ["Kobako", "Kobako::KV"] {
            assert!(
                bound(&mut catalog, path).is_err(),
                "{path:?} through Catalog::bind must be refused as reserved"
            );
        }
        assert!(
            bound(&mut catalog, "KobakoStore").is_ok(),
            "a segment only sharing the reserved prefix through Catalog::bind must bind"
        );
    }

    // The bound paths are the guest's registration input; bind order is
    // the property that matters to it.
    // @behavior S-020
    #[test]
    fn the_bound_paths_keep_bind_order() {
        let mut catalog = Catalog::default();
        bound(&mut catalog, "MyService::KV").expect("a constant path binds");
        bound(&mut catalog, "File").expect("a single segment is a whole path");
        assert_eq!(
            catalog.preamble().bindings,
            Bindings {
                paths: vec!["MyService::KV".into(), "File".into()]
            },
            "a bound catalog must hand the runtime every path in bind order"
        );
    }
}
