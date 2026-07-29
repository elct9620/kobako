//! The Extension install unit: a guest idiom paired with an optional host
//! backend, composed onto a Sandbox through the existing preload and bind
//! steps.
//!
//! The SDK twin of the Ruby gem's `Kobako::Extension`. `Sandbox::install`
//! registers an Extension's `source` as a preloaded snippet and, when it
//! carries a `Backend`, binds the backend path as a Service — a fixed object
//! for a `Static` provider, an object resolved fresh each invocation for a
//! `PerInvocation` one, or the `Unresolved` sentinel for a `Fillable` one that
//! a per-invocation `ctx.bind` override supplies. Behavior parity with the
//! Ruby frontend is pinned by the differential harness.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::error::Error;
use crate::handles::Handles;
use crate::receiver::{Fault, FaultKind, Receiver};
use crate::yielder::Yielder;

/// A backend factory invoked once per invocation. The same `Arc` shared by
/// several Extensions is resolved once per invocation to one object.
///
/// Public because `Provider::PerInvocation` carries it: a third party can
/// build one by inference, but without the name they cannot declare a
/// helper that takes or returns one.
pub type ProviderFn = Arc<dyn Fn() -> Arc<dyn Receiver> + Send + Sync>;

/// A guest idiom paired with an optional host backend — the contract
/// `Sandbox::install` consumes. Implement it on your own type; the four
/// methods mirror the Ruby `Kobako::Extension` readers. `Send + Sync`
/// because an installed Extension lives in the shared `Sandbox` an
/// `Arc<Sandbox>` drives across threads.
pub trait Extension: Send + Sync {
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

/// How a backend's bound object is sourced — the explicit spelling of the
/// three-kind choice the Ruby `Extension::Backend` makes by keyword.
pub enum Provider {
    /// One object for the Sandbox's life.
    Static(Arc<dyn Receiver>),
    /// An object resolved fresh at the start of every invocation. Provider
    /// identity is resource identity — one `Arc` shared by several
    /// Extensions resolves once per invocation to a single shared object.
    PerInvocation(ProviderFn),
    /// No object of its own: the path stays on the `Unresolved` sentinel and
    /// fails closed until a per-invocation `ctx.bind` override supplies one —
    /// the Ruby `Extension::Backend` declared with neither `object:` nor
    /// `provider:`.
    Fillable,
}

/// The sentinel backing an unresolved Service path: the install-time
/// placeholder for a `PerInvocation` backend (whose resolution runs ahead of
/// it every invocation) and the object a fillable path stays bound to until
/// the host supplies one. It reserves the path's Frame 1 slot so the guest
/// sees the constant; a dispatch that reaches it — an unfilled fillable —
/// fails closed as an undefined target, surfacing as a guest `ServiceError`.
struct Unresolved;

impl Receiver for Unresolved {
    /// Refuses before reading the payload: an unfilled fillable has no
    /// schema of its own, and the refusal is the same whatever arrived.
    fn call(
        &self,
        _method: &str,
        _payload: &[u8],
        _block: Option<&mut Yielder<'_>>,
        _handles: &Handles<'_>,
    ) -> Result<Vec<u8>, Fault> {
        Err(Fault::new(
            FaultKind::Undefined,
            "service is declared but unresolved this invocation",
        ))
    }
}

/// The `Unresolved` sentinel, bound at a fillable path so it enters Frame 1
/// and fails closed until an override fills it.
pub(crate) fn unresolved() -> Arc<dyn Receiver> {
    Arc::new(Unresolved)
}

/// The object bound at install for a backend: a `Static` provider's object
/// directly, or the `Unresolved` placeholder a `PerInvocation` provider's
/// per-invocation resolution replaces, or that a `Fillable` keeps until a
/// `ctx.bind` override fills it.
pub(crate) fn install_object(provider: &Provider) -> Arc<dyn Receiver> {
    match provider {
        Provider::Static(object) => object.clone(),
        Provider::PerInvocation(_) | Provider::Fillable => unresolved(),
    }
}

/// Per-Sandbox registry of installed Extensions. The Sandbox has already
/// composed each onto the Catalog (source preloaded, backend path bound);
/// this asserts declared dependencies at the seal and resolves each
/// `PerInvocation` backend afresh for every invocation.
#[derive(Default)]
pub(crate) struct Extensions {
    entries: Vec<Arc<dyn Extension>>,
    asserted: AtomicBool,
}

impl Extensions {
    /// Record an installed Extension.
    pub(crate) fn record(&mut self, extension: Arc<dyn Extension>) {
        self.entries.push(extension);
    }

    /// Assert every installed Extension's `depends_on` names a fellow
    /// installed Extension. Runs once, at the first successful seal; the check
    /// is presence-only, so dependency cycles are permitted. Takes `&self` so
    /// the first `eval` can run it — concurrent first invocations may each run
    /// the check, but it is pure over the sealed entries so they agree. The
    /// asserted flag flips only on success, so a seal that failed re-checks on
    /// the next attempt rather than silently passing a broken Sandbox.
    pub(crate) fn assert_dependencies(&self) -> Result<(), Error> {
        if self.asserted.load(Ordering::Relaxed) {
            return Ok(());
        }
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
        self.asserted.store(true, Ordering::Relaxed);
        Ok(())
    }

    /// Resolve each `PerInvocation` backend to this invocation's object,
    /// sharing one object per provider identity, and return the
    /// path→object pairs the dispatch handler resolves ahead of the
    /// sealed Catalog. Empty when no backend is per-invocation.
    pub(crate) fn resolve(&self) -> Vec<(String, Arc<dyn Receiver>)> {
        let mut by_provider: Vec<(ProviderFn, Arc<dyn Receiver>)> = Vec::new();
        let mut resolved = Vec::new();
        for extension in &self.entries {
            let Some(backend) = extension.backend() else {
                continue;
            };
            let Provider::PerInvocation(provider) = backend.provider else {
                continue;
            };
            let object = match by_provider
                .iter()
                .find(|(seen, _)| Arc::ptr_eq(seen, &provider))
            {
                Some((_, object)) => object.clone(),
                None => {
                    let object = provider();
                    by_provider.push((provider.clone(), object.clone()));
                    object
                }
            };
            resolved.push((backend.path, object));
        }
        resolved
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::receiver::Probe;

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
    fn assert_dependencies_re_asserts_after_a_failed_seal() {
        let mut extensions = Extensions::default();
        extensions.record(ext("File", &["Errno"]));
        assert!(extensions.assert_dependencies().is_err());
        assert!(
            extensions.assert_dependencies().is_err(),
            "a failed seal re-checks on retry; the asserted flag flips only on a successful seal"
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
    // resolution carries every path that provider backs.
    #[test]
    fn resolve_shares_one_object_across_paths_of_a_shared_provider() {
        let shared: ProviderFn = Arc::new(|| Arc::new(Probe) as Arc<dyn Receiver>);
        let resolved = resolve_of(&[("File", shared.clone()), ("Dir", shared.clone())]);
        assert_eq!(resolved.len(), 2);
        assert!(
            Arc::ptr_eq(&resolved[0].1, &resolved[1].1),
            "a shared provider backs every path with the same object"
        );
    }

    #[test]
    fn resolve_gives_distinct_providers_distinct_objects() {
        let a: ProviderFn = Arc::new(|| Arc::new(Probe) as Arc<dyn Receiver>);
        let b: ProviderFn = Arc::new(|| Arc::new(Probe) as Arc<dyn Receiver>);
        let resolved = resolve_of(&[("File", a), ("Dir", b)]);
        assert!(
            !Arc::ptr_eq(&resolved[0].1, &resolved[1].1),
            "distinct providers resolve to distinct objects"
        );
    }

    // Drive Extensions::resolve with per-invocation backends built from the
    // given (path, provider) pairs.
    fn resolve_of(specs: &[(&'static str, ProviderFn)]) -> Vec<(String, Arc<dyn Receiver>)> {
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
        extensions.resolve()
    }
}
