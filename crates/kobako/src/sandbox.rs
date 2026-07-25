//! The Sandbox: one guest, its Services, and the invocation verbs.
//!
//! The Rust counterpart of `Kobako::Sandbox`: registrations and
//! preloads fill the Catalog until the first invocation seals it, then
//! `eval` / `run` execute on a fresh guest instance and return an
//! `Execution` — the per-invocation record carrying the decoded value
//! (or a guest failure's taxonomy `Error`), the output captures, and
//! usage. Per-invocation state — the capability-Handle table and the
//! observables — lives on that returned `Execution`, not on the reused
//! Sandbox, so nothing an invocation produces outlives it on `self`.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use kobako_codec::codec::{Encode as _, Value};
use kobako_codec::payload::Arguments;
use kobako_runtime::envelope::Run;
use kobako_runtime::profile::Profile;
use kobako_runtime::runtime::{Entry, Frames, Runtime};
pub use kobako_runtime::snapshot::Usage;
use kobako_runtime::snapshot::{Completion, Snapshot};
use kobako_wasmtime::{Config, Driver};

use crate::catalog::Catalog;
use crate::dispatch::CatalogHandler;
use crate::error::{Error, GuestFailure};
use crate::execution::Execution;
use crate::extension::{install_object, unresolved, Extension, Extensions};
use crate::handles::{HandleTable, Handles};
use crate::outcome;
use crate::receiver::Receiver;
use crate::snippet;

/// Per-invocation path→object resolutions the dispatch handler answers ahead
/// of the sealed Catalog: the `ctx.bind` overrides followed by each
/// `PerInvocation` provider's fresh object.
type Resolved = Vec<(String, Arc<dyn Receiver>)>;

/// Per-Sandbox caps and posture, the counterpart of the Ruby
/// `SandboxOptions` value object. `None` means "no cap".
#[derive(Clone)]
pub struct Options {
    /// Wall-clock cap for one invocation.
    pub timeout: Option<Duration>,
    /// Guest linear-memory cap, in bytes.
    pub memory_limit: Option<usize>,
    /// Captured-stdout cap, in bytes.
    pub stdout_limit: Option<usize>,
    /// Captured-stderr cap, in bytes.
    pub stderr_limit: Option<usize>,
    /// Requested isolation floor; the driver declares its posture and
    /// construction fails below the floor.
    pub profile: Profile,
}

impl Default for Options {
    /// Secure by default: no caps, hermetic floor — the same default
    /// posture the Ruby frontend requests.
    fn default() -> Self {
        Options {
            timeout: None,
            memory_limit: None,
            stdout_limit: None,
            stderr_limit: None,
            profile: Profile::Hermetic,
        }
    }
}

/// The registration tables' seal-once lifecycle: open for setup, then
/// immutable from the first invocation on.
enum Registry {
    Open(Catalog),
    Sealed(Arc<Catalog>),
}

impl Registry {
    /// Mutate the open catalog, or refuse once sealed.
    fn open_mut(&mut self) -> Result<&mut Catalog, Error> {
        match self {
            Registry::Open(catalog) => Ok(catalog),
            Registry::Sealed(_) => Err(Error::Sealed(
                "registrations must happen before the first invocation",
            )),
        }
    }

    /// Seal on first use and hand out the shared table.
    fn seal(&mut self) -> Arc<Catalog> {
        if let Registry::Open(catalog) = self {
            let sealed = Arc::new(std::mem::take(catalog));
            *self = Registry::Sealed(sealed);
        }
        match self {
            Registry::Sealed(catalog) => catalog.clone(),
            Registry::Open(_) => unreachable!("seal above pinned the sealed state"),
        }
    }
}

/// A `run` argument: a `Value` passes by value, a host object
/// auto-wraps into a capability Handle the guest can call back into
/// (the counterpart of the Ruby `#run` auto-wrap; wrapping applies to
/// the top-level argument position).
pub enum RunArg {
    Value(Value),
    Object(Arc<dyn Receiver>),
}

impl From<Value> for RunArg {
    fn from(value: Value) -> Self {
        RunArg::Value(value)
    }
}

/// One guest sandbox: construction loads the Guest Binary and each
/// `eval` / `run` invokes it, returning that invocation's `Execution`.
/// The reused Sandbox holds only sealed config — no per-invocation
/// state — so it is the config tier and the `Execution` is the result.
pub struct Sandbox {
    driver: Driver,
    // Behind a `Mutex` so the first `eval` / `run` can seal it through
    // `&self`: setup verbs reach it with `get_mut` (they hold `&mut self`,
    // before the Sandbox is shared), the seal locks it. `Send + Sync`, so
    // an `Arc<Sandbox>` drives concurrent `&self` invocations.
    registry: Mutex<Registry>,
    extensions: Extensions,
}

impl Sandbox {
    /// Load a Guest Binary under the given caps. Fails with
    /// `Error::Setup` when the artifact is absent or unusable, or when
    /// the driver's declared posture falls below the requested floor.
    pub fn new(wasm_path: impl AsRef<Path>, options: Options) -> Result<Self, Error> {
        let config = Config {
            timeout: options.timeout,
            stdout_limit_bytes: options.stdout_limit,
            stderr_limit_bytes: options.stderr_limit,
            profile: options.profile,
        };
        let driver =
            Driver::new(wasm_path.as_ref(), options.memory_limit, config).map_err(Error::Setup)?;
        Ok(Sandbox {
            driver,
            registry: Mutex::new(Registry::Open(Catalog::default())),
            extensions: Extensions::default(),
        })
    }

    /// The open catalog for a setup mutation; refused once sealed. Setup
    /// runs on `&mut self`, before the Sandbox is shared, so it reaches the
    /// registry without locking.
    fn open_catalog(&mut self) -> Result<&mut Catalog, Error> {
        self.registry
            .get_mut()
            .expect("the registry mutex is never poisoned")
            .open_mut()
    }

    /// Bind a host object as the Service reachable at `path` — a
    /// constant path of one or more `::`-separated segments
    /// (`"MyService::KV"` or a top-level `"File"`). Refused once sealed.
    pub fn bind(&mut self, path: &str, object: Arc<dyn Receiver>) -> Result<(), Error> {
        self.open_catalog()?.bind(path, object);
        Ok(())
    }

    /// Declare a fillable Service at `path` with no object — the Rust spelling
    /// of the Ruby frontend's `bind(path)`. The path enters Frame 1 so the
    /// guest sees the constant, but is backed by the unresolved sentinel until
    /// an override fills it; a guest dispatch to an unfilled fillable fails
    /// closed as an undefined target (a guest `ServiceError`). Refused once
    /// sealed.
    pub fn bind_fillable(&mut self, path: &str) -> Result<(), Error> {
        self.open_catalog()?.bind(path, unresolved());
        Ok(())
    }

    /// Install an Extension — a guest idiom (`source`) paired with an
    /// optional host backend — composing it over `preload` and `bind`. A
    /// `Static` provider binds its object directly; a `PerInvocation`
    /// provider is resolved fresh at every invocation. Refused once the
    /// first invocation seals registration; an Extension whose `depends_on`
    /// names one that was not installed surfaces at that first invocation.
    pub fn install(&mut self, extension: Arc<dyn Extension>) -> Result<(), Error> {
        let catalog = self.open_catalog()?;
        catalog
            .snippets
            .register_source(extension.name(), extension.source())?;
        if let Some(backend) = extension.backend() {
            catalog.bind(&backend.path, install_object(&backend.provider));
        }
        self.extensions.record(extension);
        Ok(())
    }

    /// Register a source snippet for per-invocation replay under its
    /// canonical backtrace name. Refused once sealed, on a
    /// non-constant name, or on a duplicate name.
    pub fn preload(&mut self, name: &str, source: &str) -> Result<(), Error> {
        self.open_catalog()?.snippets.register_source(name, source)
    }

    /// Register precompiled RITE bytecode for per-invocation replay.
    /// The bytes stay opaque host-side; the guest validates them at
    /// first replay. Refused once sealed.
    pub fn preload_binary(&mut self, bytecode: impl Into<Vec<u8>>) -> Result<(), Error> {
        self.open_catalog()?
            .snippets
            .register_binary(bytecode.into());
        Ok(())
    }

    /// Run one mruby source on a fresh guest instance and return its
    /// `Execution` — the record of the run. `Ok` means the guest ran (its
    /// last expression, or a guest failure, rides `Execution::value`); the
    /// outer `Err` means it never started.
    pub fn eval(&self, source: &str) -> Result<Execution, Error> {
        let (catalog, handles) = self.begin_invocation()?;
        self.invoke(
            catalog,
            handles,
            Entry::Eval {
                source: source.as_bytes(),
            },
            Vec::new(),
        )
    }

    /// `eval` with a per-invocation override closure — the Rust spelling of
    /// the Ruby frontend's `#eval { |ctx| ctx.bind(...) }`. The closure runs
    /// before the guest drives, receiving the per-invocation `Context` whose
    /// `bind` fills a fillable or shadows any declared binding for this
    /// invocation only; overriding an undeclared path returns `Error::Argument`
    /// before the guest runs. An override takes priority over that path's
    /// per-invocation provider result and its static base, and touches
    /// host-side resolution only — Frame 1 stays fixed.
    pub fn eval_with<F>(&self, source: &str, overrides: F) -> Result<Execution, Error>
    where
        F: FnOnce(&mut Context<'_>) -> Result<(), Error>,
    {
        let (catalog, handles) = self.begin_invocation()?;
        let resolved = collect_overrides(&catalog, overrides)?;
        self.invoke(
            catalog,
            handles,
            Entry::Eval {
                source: source.as_bytes(),
            },
            resolved,
        )
    }

    /// Dispatch into a preloaded entrypoint with positional and keyword
    /// arguments; the guest resolves `target` as a top-level constant and
    /// invokes its `call`. A `RunArg::Object` argument auto-wraps into a
    /// capability Handle before the envelope encodes. Host pre-flight refuses a
    /// non-constant `target` before the invocation seals the tables, matching
    /// the Ruby frontend's ordering.
    pub fn run(
        &self,
        target: &str,
        args: Vec<RunArg>,
        kwargs: Vec<(String, RunArg)>,
    ) -> Result<Execution, Error> {
        self.drive_run(target, args, kwargs, |_| Ok(Vec::new()))
    }

    /// `run` with a per-invocation override closure — the Rust spelling of the
    /// Ruby frontend's `#run(target, ...) { |ctx| ctx.bind(...) }`, the `run`
    /// counterpart of `eval_with`. The closure runs before the guest drives and
    /// binds overrides under the same rules `eval_with` documents.
    pub fn run_with<F>(
        &self,
        target: &str,
        args: Vec<RunArg>,
        kwargs: Vec<(String, RunArg)>,
        overrides: F,
    ) -> Result<Execution, Error>
    where
        F: FnOnce(&mut Context<'_>) -> Result<(), Error>,
    {
        self.drive_run(target, args, kwargs, move |catalog| {
            collect_overrides(catalog, overrides)
        })
    }

    /// Shared `run` / `run_with` core: validate the target before sealing, seal,
    /// collect any overrides against the sealed catalog, auto-wrap the args into
    /// this run's Handle table, and drive the entrypoint envelope. `collect`
    /// yields the per-invocation overrides — empty for `run`, the closure's for
    /// `run_with`.
    fn drive_run<C>(
        &self,
        target: &str,
        args: Vec<RunArg>,
        kwargs: Vec<(String, RunArg)>,
        collect: C,
    ) -> Result<Execution, Error>
    where
        C: FnOnce(&Catalog) -> Result<Resolved, Error>,
    {
        if !snippet::constant_name(target) {
            return Err(Error::Argument(format!(
                "entrypoint must be a Ruby constant name (got {target:?})"
            )));
        }
        let (catalog, handles) = self.begin_invocation()?;
        let resolved = collect(&catalog)?;
        let args = args
            .into_iter()
            .map(|arg| wrap_run_arg(&handles, arg))
            .collect::<Result<_, _>>()?;
        let kwargs = kwargs
            .into_iter()
            .map(|(key, arg)| Ok((key, wrap_run_arg(&handles, arg)?)))
            .collect::<Result<_, Error>>()?;
        let payload = Arguments::new(args, kwargs)
            .encode()
            .map_err(|err| Error::Argument(format!("arguments are not wire-encodable: {err}")))?;
        let envelope = Run {
            entrypoint: target.to_string(),
            payload,
        }
        .encode();
        self.invoke(
            catalog,
            handles,
            Entry::Run {
                envelope: &envelope,
            },
            resolved,
        )
    }

    /// Shared invocation core behind `eval` / `run`: assemble the
    /// sealed catalog's frames and dispatch handler over this invocation's
    /// fresh Handle table, drive `entry` through the driver, and cook the
    /// snapshot into an `Execution` — one owner for the wiring so a handler
    /// or frame change cannot drift between verbs. `&self` because no
    /// per-invocation state is written back: the `handles` table and the
    /// snapshot's observables ride into the returned `Execution`.
    /// `resolved` starts with the per-invocation `ctx.bind` overrides so the
    /// dispatch handler answers them before this run's provider results.
    fn invoke(
        &self,
        catalog: Arc<Catalog>,
        handles: Arc<Mutex<HandleTable>>,
        entry: Entry<'_>,
        mut resolved: Resolved,
    ) -> Result<Execution, Error> {
        resolved.extend(self.extensions.resolve());
        let preamble = catalog.preamble();
        let snippets = catalog.snippets.frame();
        let handler = Arc::new(CatalogHandler::new(catalog, handles.clone(), resolved));
        let snapshot = self.driver.invoke(
            entry,
            Frames {
                preamble: &preamble,
                snippets: &snippets,
            },
            Some(handler),
        )?;
        Ok(build_execution(snapshot, handles))
    }

    /// Per-invocation prologue on `&self`: seal the registration tables and
    /// assert Extension dependencies on the first invocation, then hand back
    /// the sealed catalog and a fresh Handle table this invocation owns. The
    /// seal locks the registry, so concurrent first invocations serialize on
    /// it and all observe the same sealed catalog. An unmet dependency raises
    /// before the guest runs.
    fn begin_invocation(&self) -> Result<(Arc<Catalog>, Arc<Mutex<HandleTable>>), Error> {
        let catalog = self
            .registry
            .lock()
            .expect("the registry mutex is never poisoned")
            .seal();
        self.extensions.assert_dependencies()?;
        Ok((catalog, Arc::new(Mutex::new(HandleTable::default()))))
    }
}

/// Cook a raw `Snapshot` into the invocation's `Execution`: captures and
/// usage carry over verbatim, and the completion becomes the guest-level
/// `outcome` — a decoded value (whose Handles must all be live), or the
/// taxonomy `Error` a trap or guest failure attributes to. The `handles`
/// table rides along so the result's Handles resolve on the Execution.
fn build_execution(snapshot: Snapshot, handles: Arc<Mutex<HandleTable>>) -> Execution {
    let outcome = match snapshot.completion {
        Completion::Outcome(bytes) => outcome::decode(&bytes).and_then(|value| {
            require_live_handles(&handles, &value)?;
            Ok(value)
        }),
        Completion::Trap(trap) => Err(trap.into()),
    };
    Execution::new(
        outcome,
        handles,
        snapshot.stdout,
        snapshot.stderr,
        snapshot.usage,
    )
}

/// Encode one `run` argument, auto-wrapping a host object into the
/// invocation's Handle table. Exhaustion surfaces pre-call with the Ruby
/// counterpart's attribution — an outer `Err`, since the guest never ran.
fn wrap_run_arg(handles: &Mutex<HandleTable>, arg: RunArg) -> Result<Value, Error> {
    match arg {
        RunArg::Value(value) => Ok(value),
        RunArg::Object(object) => handles
            .lock()
            .expect("the Handle table mutex is never poisoned")
            .alloc(object)
            .map(Value::Handle)
            .map_err(|message| {
                Error::Sandbox(GuestFailure {
                    class: "Kobako::HandleExhaustedError".into(),
                    message,
                    backtrace: Vec::new(),
                    details: None,
                })
            }),
    }
}

/// Every Handle a guest legitimately returns resolves to a live object
/// (it cannot fabricate one); an unknown id in the result signals a
/// corrupted runtime and fails like a malformed value.
fn require_live_handles(handles: &Mutex<HandleTable>, value: &Value) -> Result<(), Error> {
    match value {
        Value::Handle(id) => {
            if Handles::new(handles).resolve(value).is_some() {
                Ok(())
            } else {
                Err(Error::Sandbox(GuestFailure {
                    class: "Kobako::SandboxError".into(),
                    message: format!("unknown Handle id: {id}"),
                    backtrace: Vec::new(),
                    details: None,
                }))
            }
        }
        Value::Array(items) => items
            .iter()
            .try_for_each(|v| require_live_handles(handles, v)),
        Value::Map(pairs) => pairs.iter().try_for_each(|(key, val)| {
            require_live_handles(handles, key)?;
            require_live_handles(handles, val)
        }),
        _ => Ok(()),
    }
}

/// Run an override closure against a fresh `Context` over `catalog` and hand
/// back the `ctx.bind` overrides it collected — the step `eval_with` and
/// `run_with` share so the two verbs bind overrides identically.
fn collect_overrides<F>(catalog: &Catalog, overrides: F) -> Result<Resolved, Error>
where
    F: FnOnce(&mut Context<'_>) -> Result<(), Error>,
{
    let mut ctx = Context {
        catalog,
        overrides: Vec::new(),
    };
    overrides(&mut ctx)?;
    Ok(ctx.overrides)
}

/// The per-invocation Context handed to an `eval_with` / `run_with` override closure — the
/// Rust peer of the Ruby frontend's `Kobako::Context`. Here it carries only the
/// `ctx.bind` overrides; the run's Handle table and observables ride into its
/// `Execution` instead. Each override takes priority over that path's
/// per-invocation provider result and its static base for that one invocation,
/// and is discarded when it ends. The borrow keeps it from outliving the
/// closure, which is what spends it.
pub struct Context<'a> {
    catalog: &'a Catalog,
    overrides: Resolved,
}

impl Context<'_> {
    /// Override the object bound at an already-declared `path` for this
    /// invocation — filling a fillable or shadowing a static / per-invocation
    /// binding. Overriding a path that was never declared returns
    /// `Error::Argument`, so an override can never grow the sealed key set.
    /// A second override of the same `path` wins over the first, matching the
    /// Ruby frontend's last-wins semantics — an override is the caller's final
    /// word on that path for the run.
    pub fn bind(&mut self, path: &str, object: Arc<dyn Receiver>) -> Result<(), Error> {
        if self.catalog.lookup(path).is_none() {
            return Err(Error::Argument(format!(
                "cannot override undeclared path {path:?}"
            )));
        }
        self.overrides.retain(|(bound, _)| bound != path);
        self.overrides.push((path.to_string(), object));
        Ok(())
    }
}

/// `Arc<Sandbox>` drives concurrent `&self` invocations, which requires
/// `Sandbox: Send + Sync`. Assert it at compile time so a future field that
/// breaks the property fails the build here, not at a distant call site.
const _: fn() = || {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Sandbox>();
};

#[cfg(test)]
mod tests {
    use super::*;

    // The seal-once lifecycle is pure state and testable without a
    // driver; the invocation path itself is pinned end-to-end by the
    // parity harness against the real guest binary.
    #[test]
    fn registry_seals_once_and_refuses_late_mutation() {
        let mut registry = Registry::Open(Catalog::default());
        assert!(registry.open_mut().is_ok());
        let first = registry.seal();
        let second = registry.seal();
        assert!(Arc::ptr_eq(&first, &second));
        assert!(matches!(registry.open_mut(), Err(Error::Sealed(_))));
    }
}
