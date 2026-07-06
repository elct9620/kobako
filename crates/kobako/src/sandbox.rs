//! The Sandbox: one guest, its Services, and the invocation verbs.
//!
//! The Rust counterpart of `Kobako::Sandbox`: registrations and
//! preloads fill the Catalog until the first invocation seals it,
//! `eval` / `run` execute on a fresh guest instance and yield the
//! decoded value or a taxonomy `Error`, and the capture / usage
//! readers expose the per-invocation observables. The
//! capability-Handle table is a seam of a later build.

use std::path::Path;
use std::sync::Arc;
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
use crate::error::Error;
use crate::member::Member;
use crate::outcome;
use crate::snippet;

/// Per-Sandbox caps and posture, the counterpart of the Ruby
/// `SandboxOptions` value object. `None` means "no cap".
#[derive(Clone)]
pub struct Options {
    pub timeout: Option<Duration>,
    pub memory_limit: Option<usize>,
    pub stdout_limit: Option<usize>,
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

/// One guest sandbox: construction loads the Guest Binary, `eval`
/// invokes it, and the readers expose the last invocation's
/// observables.
pub struct Sandbox {
    driver: Driver,
    registry: Registry,
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
            stdout: empty_capture(),
            stderr: empty_capture(),
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
        object: Arc<dyn Member>,
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
        let catalog = self.registry.seal();
        let preamble = catalog.preamble();
        let snippets = catalog.snippets.frame();
        let handler = Arc::new(CatalogHandler::new(catalog));
        let snapshot = self.driver.invoke(
            Entry::Eval {
                source: source.as_bytes(),
            },
            Frames {
                preamble: &preamble,
                snippets: &snippets,
            },
            Some(handler),
        )?;
        self.read_snapshot(snapshot)
    }

    /// Dispatch into a preloaded entrypoint without arguments; the
    /// guest resolves `target` as a top-level constant and invokes its
    /// `call`.
    pub fn run(&mut self, target: &str) -> Result<Value, Error> {
        self.run_with(target, Vec::new(), Vec::new())
    }

    /// Dispatch into a preloaded entrypoint with positional and
    /// keyword arguments. Host pre-flight refuses a non-constant
    /// `target` before the invocation seals the tables, matching the
    /// Ruby frontend's ordering.
    pub fn run_with(
        &mut self,
        target: &str,
        args: Vec<Value>,
        kwargs: Vec<(String, Value)>,
    ) -> Result<Value, Error> {
        if !snippet::constant_name(target) {
            return Err(Error::Argument(format!(
                "entrypoint must be a Ruby constant name (got {target:?})"
            )));
        }
        let catalog = self.registry.seal();
        let preamble = catalog.preamble();
        let snippets = catalog.snippets.frame();
        let envelope = Run {
            entrypoint: target.to_string(),
            args,
            kwargs,
        }
        .encode()
        .map_err(|err| Error::Argument(format!("arguments are not wire-encodable: {err}")))?;
        let handler = Arc::new(CatalogHandler::new(catalog));
        let snapshot = self.driver.invoke(
            Entry::Run {
                envelope: &envelope,
            },
            Frames {
                preamble: &preamble,
                snippets: &snippets,
            },
            Some(handler),
        )?;
        self.read_snapshot(snapshot)
    }

    /// Stash the invocation's observables, then classify its
    /// completion: captures and usage survive traps.
    fn read_snapshot(&mut self, snapshot: Snapshot) -> Result<Value, Error> {
        self.stdout = snapshot.stdout;
        self.stderr = snapshot.stderr;
        self.usage = Some(snapshot.usage);
        match snapshot.completion {
            Completion::Outcome(bytes) => outcome::decode(&bytes),
            Completion::Trap(trap) => Err(trap.into()),
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

fn empty_capture() -> Capture {
    Capture {
        bytes: Vec::new(),
        truncated: false,
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
