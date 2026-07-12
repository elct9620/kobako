//! The Extension install unit: a guest idiom paired with an optional host
//! backend, composed onto a Sandbox through the existing preload and bind
//! steps.
//!
//! The SDK twin of the Ruby gem's `Kobako::Extension`. `Sandbox::install`
//! registers an Extension's `source` as a preloaded snippet and, when it
//! carries a `Backend`, binds the backend path as a Service — a fixed
//! object for a `Static` provider, or an object resolved fresh each
//! invocation for a `PerInvocation` one. Behavior parity with the Ruby
//! frontend is pinned by the differential harness.

use std::sync::Arc;

use kobako_codec::codec::Value;

use crate::error::Error;
use crate::handles::Handles;
use crate::receiver::{Fault, FaultKind, Receiver};
use crate::yielder::Yielder;

/// A backend factory invoked once per invocation. The same `Arc` shared by
/// several Extensions is resolved once per invocation to one object.
type ProviderFn = Arc<dyn Fn() -> Arc<dyn Receiver> + Send + Sync>;

/// A guest idiom paired with an optional host backend — the contract
/// `Sandbox::install` consumes. Implement it on your own type; the four
/// methods mirror the Ruby `Kobako::Extension` readers.
pub trait Extension {
    /// Identity: the preloaded snippet's canonical name and the
    /// `depends_on` match key, a Ruby constant token. Independent of any
    /// bound path.
    fn name(&self) -> &str;

    /// The mruby idiom, preloaded as a snippet.
    fn source(&self) -> &str;

    /// Names of Extensions that must also be installed; asserted for
    /// presence at the first invocation. Empty by default.
    fn depends_on(&self) -> &[&str] {
        &[]
    }

    /// The host attachment — a path plus a provider — or `None` for a
    /// pure-guest Extension.
    fn backend(&self) -> Option<Backend> {
        None
    }
}

/// The host attachment of an Extension: the constant `path` the backend
/// binds at, paired with the `provider` that sources its object.
pub struct Backend {
    pub path: String,
    pub provider: Provider,
}

/// How a backend's bound object is sourced.
pub enum Provider {
    /// One object for the Sandbox's life.
    Static(Arc<dyn Receiver>),
    /// An object resolved fresh at the start of every invocation. Provider
    /// identity is resource identity — one `Arc` shared by several
    /// Extensions resolves once per invocation to a single shared object.
    PerInvocation(ProviderFn),
}

/// Placeholder bound at install for a `PerInvocation` backend so its path
/// enters the Frame 1 preamble; the per-invocation overlay always resolves
/// ahead of it, so a dispatch never reaches this.
struct Unresolved;

impl Receiver for Unresolved {
    fn call(
        &self,
        _method: &str,
        _args: &[Value],
        _kwargs: &[(String, Value)],
        _block: Option<&mut Yielder<'_>>,
        _handles: &Handles<'_>,
    ) -> Result<Value, Fault> {
        Err(Fault::new(
            FaultKind::Undefined,
            "extension backend not resolved for this invocation",
        ))
    }
}

/// The object bound at install for a backend: a `Static` provider's object
/// directly, or the `Unresolved` placeholder a `PerInvocation` provider's
/// overlay replaces.
pub(crate) fn install_object(provider: &Provider) -> Arc<dyn Receiver> {
    match provider {
        Provider::Static(object) => object.clone(),
        Provider::PerInvocation(_) => Arc::new(Unresolved),
    }
}

/// Per-Sandbox registry of installed Extensions. The Sandbox has already
/// composed each onto the Catalog (source preloaded, backend path bound);
/// this asserts declared dependencies at the seal and produces the
/// per-invocation overlay resolving each `PerInvocation` backend.
#[derive(Default)]
pub(crate) struct Extensions {
    entries: Vec<Arc<dyn Extension>>,
    asserted: bool,
}

impl Extensions {
    /// Record an installed Extension.
    pub(crate) fn record(&mut self, extension: Arc<dyn Extension>) {
        self.entries.push(extension);
    }

    /// Assert every installed Extension's `depends_on` names a fellow
    /// installed Extension. Runs once, at the first seal; the check is
    /// presence-only, so dependency cycles are permitted.
    pub(crate) fn assert_dependencies(&mut self) -> Result<(), Error> {
        if self.asserted {
            return Ok(());
        }
        self.asserted = true;
        let names: Vec<&str> = self.entries.iter().map(|entry| entry.name()).collect();
        for extension in &self.entries {
            for dependency in extension.depends_on() {
                if !names.contains(dependency) {
                    return Err(Error::Argument(format!(
                        "extension {:?} depends on {:?}, which was not installed",
                        extension.name(),
                        dependency
                    )));
                }
            }
        }
        Ok(())
    }

    /// Resolve each `PerInvocation` backend to this invocation's object,
    /// sharing one object per provider identity, and return the
    /// path→object overlay the dispatch handler resolves ahead of the
    /// sealed Catalog. Empty when no backend is per-invocation.
    pub(crate) fn overlay(&self) -> Vec<(String, Arc<dyn Receiver>)> {
        let mut resolved: Vec<(ProviderFn, Arc<dyn Receiver>)> = Vec::new();
        let mut overlay = Vec::new();
        for extension in &self.entries {
            let Some(backend) = extension.backend() else {
                continue;
            };
            let Provider::PerInvocation(provider) = backend.provider else {
                continue;
            };
            let object = match resolved
                .iter()
                .find(|(seen, _)| Arc::ptr_eq(seen, &provider))
            {
                Some((_, object)) => object.clone(),
                None => {
                    let object = provider();
                    resolved.push((provider.clone(), object.clone()));
                    object
                }
            };
            overlay.push((backend.path, object));
        }
        overlay
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Probe;

    impl Receiver for Probe {
        fn call(
            &self,
            _method: &str,
            _args: &[Value],
            _kwargs: &[(String, Value)],
            _block: Option<&mut Yielder<'_>>,
            _handles: &Handles<'_>,
        ) -> Result<Value, Fault> {
            Ok(Value::Nil)
        }
    }

    struct TestExt {
        name: &'static str,
        depends_on: &'static [&'static str],
    }

    impl Extension for TestExt {
        fn name(&self) -> &str {
            self.name
        }

        fn source(&self) -> &str {
            "1"
        }

        fn depends_on(&self) -> &[&str] {
            self.depends_on
        }
    }

    fn ext(name: &'static str, depends_on: &'static [&'static str]) -> Arc<dyn Extension> {
        Arc::new(TestExt { name, depends_on })
    }

    #[test]
    fn assert_dependencies_accepts_a_satisfied_set() {
        let mut extensions = Extensions::default();
        extensions.record(ext("Errno", &[]));
        extensions.record(ext("File", &["Errno"]));
        assert!(extensions.assert_dependencies().is_ok());
    }

    #[test]
    fn assert_dependencies_rejects_an_unmet_dependency() {
        let mut extensions = Extensions::default();
        extensions.record(ext("File", &["Errno"]));
        let err = extensions.assert_dependencies().unwrap_err();
        assert!(
            matches!(err, Error::Argument(message) if message.contains("File") && message.contains("Errno")),
            "an unmet dependency names both ends"
        );
    }

    #[test]
    fn assert_dependencies_permits_cycles() {
        let mut extensions = Extensions::default();
        extensions.record(ext("A", &["B"]));
        extensions.record(ext("B", &["A"]));
        assert!(extensions.assert_dependencies().is_ok());
    }

    // A shared provider resolves once per invocation to one object; the
    // overlay carries every path that provider backs.
    #[test]
    fn overlay_shares_one_object_across_paths_of_a_shared_provider() {
        let shared: ProviderFn = Arc::new(|| Arc::new(Probe) as Arc<dyn Receiver>);
        let overlay = overlay_of(&[("File", shared.clone()), ("Dir", shared.clone())]);
        assert_eq!(overlay.len(), 2);
        assert!(
            Arc::ptr_eq(&overlay[0].1, &overlay[1].1),
            "a shared provider backs every path with the same object"
        );
    }

    #[test]
    fn overlay_gives_distinct_providers_distinct_objects() {
        let a: ProviderFn = Arc::new(|| Arc::new(Probe) as Arc<dyn Receiver>);
        let b: ProviderFn = Arc::new(|| Arc::new(Probe) as Arc<dyn Receiver>);
        let overlay = overlay_of(&[("File", a), ("Dir", b)]);
        assert!(
            !Arc::ptr_eq(&overlay[0].1, &overlay[1].1),
            "distinct providers resolve to distinct objects"
        );
    }

    // Drive Extensions::overlay with per-invocation backends built from the
    // given (path, provider) pairs.
    fn overlay_of(specs: &[(&'static str, ProviderFn)]) -> Vec<(String, Arc<dyn Receiver>)> {
        struct BackendExt {
            path: &'static str,
            provider: ProviderFn,
        }
        impl Extension for BackendExt {
            fn name(&self) -> &str {
                self.path
            }
            fn source(&self) -> &str {
                "1"
            }
            fn backend(&self) -> Option<Backend> {
                Some(Backend {
                    path: self.path.to_string(),
                    provider: Provider::PerInvocation(self.provider.clone()),
                })
            }
        }
        let mut extensions = Extensions::default();
        for (path, provider) in specs {
            extensions.record(Arc::new(BackendExt {
                path,
                provider: provider.clone(),
            }));
        }
        extensions.overlay()
    }
}
