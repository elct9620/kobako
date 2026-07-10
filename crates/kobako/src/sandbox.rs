//! The Sandbox: one guest, its Services, and the invocation verbs.
//!
//! The Rust counterpart of `Kobako::Sandbox`: registrations and
//! preloads fill the Catalog until the first invocation seals it,
//! `eval` / `run` execute on a fresh guest instance and yield the
//! decoded value or a taxonomy `Error`, and the capture / usage
//! readers expose the per-invocation observables. The Sandbox also
//! owns the capability-Handle table: it resets at every invocation
//! entry, and `resolve` turns a `Value::Handle` in the result back
//! into the live host object it stands for.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use kobako_codec::codec::{Encode as _, Value};
use kobako_codec::transport::Run;
use kobako_runtime::profile::Profile;
use kobako_runtime::runtime::{Entry, Frames, Runtime};
pub use kobako_runtime::snapshot::Usage;
use kobako_runtime::snapshot::{Capture, Completion, Snapshot};
use kobako_wasmtime::{Config, Driver};

use crate::catalog::Catalog;
use crate::dispatch::CatalogHandler;
use crate::error::{Error, GuestFailure};
use crate::handles::{HandleTable, Handles};
use crate::outcome;
use crate::receiver::Receiver;
use crate::snippet;

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

/// One guest sandbox: construction loads the Guest Binary, `eval`
/// invokes it, and the readers expose the last invocation's
/// observables.
pub struct Sandbox {
    driver: Driver,
    registry: Registry,
    handles: Arc<Mutex<HandleTable>>,
    stdout: Capture,
    stderr: Capture,
    usage: Option<Usage>,
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
            registry: Registry::Open(Catalog::default()),
            handles: Arc::default(),
            stdout: Capture::default(),
            stderr: Capture::default(),
            usage: None,
        })
    }

    /// Declare a Namespace (idempotent). Refused once sealed.
    pub fn define(&mut self, namespace: &str) -> Result<(), Error> {
        self.registry.open_mut()?.define(namespace);
        Ok(())
    }

    /// Bind a host object as `<namespace>::<member>`, declaring the
    /// Namespace when absent. Refused once sealed.
    pub fn bind(
        &mut self,
        namespace: &str,
        member: &str,
        object: Arc<dyn Receiver>,
    ) -> Result<(), Error> {
        self.registry.open_mut()?.bind(namespace, member, object);
        Ok(())
    }

    /// Register a source snippet for per-invocation replay under its
    /// canonical backtrace name. Refused once sealed, on a
    /// non-constant name, or on a duplicate name.
    pub fn preload(&mut self, name: &str, source: &str) -> Result<(), Error> {
        self.registry
            .open_mut()?
            .snippets
            .register_source(name, source)
    }

    /// Register precompiled RITE bytecode for per-invocation replay.
    /// The bytes stay opaque host-side; the guest validates them at
    /// first replay. Refused once sealed.
    pub fn preload_binary(&mut self, bytecode: impl Into<Vec<u8>>) -> Result<(), Error> {
        self.registry
            .open_mut()?
            .snippets
            .register_binary(bytecode.into());
        Ok(())
    }

    /// Run one mruby source on a fresh guest instance and return its
    /// last expression as a decoded wire `Value`.
    pub fn eval(&mut self, source: &str) -> Result<Value, Error> {
        let catalog = self.begin_invocation();
        self.invoke(
            catalog,
            Entry::Eval {
                source: source.as_bytes(),
            },
        )
    }

    /// Dispatch into a preloaded entrypoint without arguments; the
    /// guest resolves `target` as a top-level constant and invokes its
    /// `call`.
    pub fn run(&mut self, target: &str) -> Result<Value, Error> {
        self.run_with(target, Vec::new(), Vec::new())
    }

    /// Dispatch into a preloaded entrypoint with positional and
    /// keyword arguments. A `RunArg::Object` argument auto-wraps into
    /// a capability Handle before the envelope encodes. Host
    /// pre-flight refuses a non-constant `target` before the
    /// invocation seals the tables, matching the Ruby frontend's
    /// ordering.
    pub fn run_with(
        &mut self,
        target: &str,
        args: Vec<RunArg>,
        kwargs: Vec<(String, RunArg)>,
    ) -> Result<Value, Error> {
        if !snippet::constant_name(target) {
            return Err(Error::Argument(format!(
                "entrypoint must be a Ruby constant name (got {target:?})"
            )));
        }
        let catalog = self.begin_invocation();
        let args = args
            .into_iter()
            .map(|arg| self.wrap_run_arg(arg))
            .collect::<Result<_, _>>()?;
        let kwargs = kwargs
            .into_iter()
            .map(|(key, arg)| Ok((key, self.wrap_run_arg(arg)?)))
            .collect::<Result<_, Error>>()?;
        let envelope = Run {
            entrypoint: target.to_string(),
            args,
            kwargs,
        }
        .encode()
        .map_err(|err| Error::Argument(format!("arguments are not wire-encodable: {err}")))?;
        self.invoke(
            catalog,
            Entry::Run {
                envelope: &envelope,
            },
        )
    }

    /// Shared invocation core behind `eval` / `run_with`: assemble the
    /// sealed catalog's frames and dispatch handler, drive `entry`
    /// through the driver, and read the snapshot — one owner for the
    /// wiring so a handler or frame change cannot drift between verbs.
    fn invoke(&mut self, catalog: Arc<Catalog>, entry: Entry<'_>) -> Result<Value, Error> {
        let preamble = catalog.preamble();
        let snippets = catalog.snippets.frame();
        let handler = Arc::new(CatalogHandler::new(catalog, self.handles.clone()));
        let snapshot = self.driver.invoke(
            entry,
            Frames {
                preamble: &preamble,
                snippets: &snippets,
            },
            Some(handler),
        )?;
        self.read_snapshot(snapshot)
    }

    /// Resolve a `Value::Handle` from the last invocation's result to
    /// the live host object it stands for — the Rust spelling of the
    /// Ruby frontend's restore-to-original-object; upcast the `Arc` to
    /// `Arc<dyn Any + Send + Sync>` and `downcast` to recover the
    /// concrete type. `None` for a non-Handle value; the table stays
    /// readable until the next invocation resets it.
    pub fn resolve(&self, value: &Value) -> Option<Arc<dyn Receiver>> {
        Handles::new(&self.handles).resolve(value)
    }

    /// Per-invocation prologue: seal the registration tables and clear
    /// the Handle table so no Handle survives the boundary.
    fn begin_invocation(&mut self) -> Arc<Catalog> {
        self.handles
            .lock()
            .expect("the Handle table mutex is never poisoned")
            .reset();
        self.registry.seal()
    }

    /// Encode one `run` argument, auto-wrapping a host object into the
    /// invocation's Handle table. Exhaustion surfaces pre-call with
    /// the Ruby counterpart's attribution.
    fn wrap_run_arg(&self, arg: RunArg) -> Result<Value, Error> {
        match arg {
            RunArg::Value(value) => Ok(value),
            RunArg::Object(object) => self
                .handles
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

    /// Stash the invocation's observables, then classify its
    /// completion: captures and usage survive traps.
    fn read_snapshot(&mut self, snapshot: Snapshot) -> Result<Value, Error> {
        self.stdout = snapshot.stdout;
        self.stderr = snapshot.stderr;
        self.usage = Some(snapshot.usage);
        match snapshot.completion {
            Completion::Outcome(bytes) => {
                let value = outcome::decode(&bytes)?;
                self.require_live_handles(&value)?;
                Ok(value)
            }
            Completion::Trap(trap) => Err(trap.into()),
        }
    }

    /// Every Handle a guest legitimately returns resolves to a live
    /// object (it cannot fabricate one); an unknown id in the result
    /// signals a corrupted runtime and fails like a malformed value.
    fn require_live_handles(&self, value: &Value) -> Result<(), Error> {
        match value {
            Value::Handle(id) => {
                if self.resolve(value).is_some() {
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
            Value::Array(items) => items.iter().try_for_each(|v| self.require_live_handles(v)),
            Value::Map(pairs) => pairs.iter().try_for_each(|(key, val)| {
                self.require_live_handles(key)?;
                self.require_live_handles(val)
            }),
            _ => Ok(()),
        }
    }

    /// Bytes the guest wrote to `$stdout` during the last invocation.
    pub fn stdout(&self) -> &[u8] {
        &self.stdout.bytes
    }

    /// Bytes the guest wrote to `$stderr` during the last invocation.
    pub fn stderr(&self) -> &[u8] {
        &self.stderr.bytes
    }

    /// Whether the stdout cap clipped the last invocation's output.
    pub fn stdout_truncated(&self) -> bool {
        self.stdout.truncated
    }

    /// Whether the stderr cap clipped the last invocation's output.
    pub fn stderr_truncated(&self) -> bool {
        self.stderr.truncated
    }

    /// Resource usage of the last invocation; `None` before any.
    pub fn usage(&self) -> Option<Usage> {
        self.usage
    }
}

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
