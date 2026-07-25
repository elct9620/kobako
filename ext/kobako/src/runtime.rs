//! Host-side magnus shell over the extracted wasmtime driver.
//!
//! The Ruby-visible classes are
//!
//!   Kobako::Runtime           — wraps a `kobako_wasmtime::Driver`
//!   Kobako::Runtime::Snapshot — one invocation's completion + captures + usage
//!
//! `Kobako::Runtime` is constructed via `Kobako::Runtime.from_path(path,
//! timeout, memory_limit, stdout_limit, stderr_limit, profile)`. Every
//! invocation (`#eval` / `#run`) takes that run's dispatch handler as a call
//! argument, instantiates a fresh instance, and returns a `Snapshot` — the
//! whole per-invocation result — so the Runtime holds no per-invocation
//! state and one Runtime is safe to drive concurrently. The run mechanics —
//! engine/module caches, caps, trap classification — live in the
//! `kobako-wasmtime` crate behind the `kobako_runtime` contract; no wasm
//! engine type reaches this crate or the Host App.
//!
//! Module layout — one responsibility per file:
//!
//! * `bridge` — the magnus dispatch bridge: `RubyDispatchHandler` plus the
//!   frame-scoped `GuestYielder` Ruby class.
//! * `errors` — the single boundary mapping the neutral `Trap` /
//!   `SetupError` channels onto the `Kobako::*` classes for a failure that
//!   never produced a `Snapshot` (a could-not-start fault).

mod bridge;
mod errors;
mod gvl;

use magnus::{
    function, method, prelude::*, typed_data::DataTypeFunctions, value::Opaque,
    Error as MagnusError, RArray, RModule, RString, Ruby, Symbol, TypedData, Value,
};

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use kobako_runtime::dispatch::DispatchHandler;
use kobako_runtime::envelope::{Preamble, Snippet, Snippets};
use kobako_runtime::error::Trap;
use kobako_runtime::profile::Profile;
use kobako_runtime::runtime::{Entry, Frames, Runtime as ContractRuntime};
use kobako_runtime::snapshot::{Capture, Completion, Snapshot as RuntimeSnapshot, Usage};
use kobako_wasmtime::{Config, Driver};

/// Copy the bytes of `s` into a fresh `Vec<u8>`. Single safe entry to
/// what would otherwise be an inline `unsafe { rstring.as_slice() }
/// .to_vec()` duplicated at every host-↔-guest boundary. The borrow
/// does not outlive this call, so no Ruby allocation can move the
/// underlying RString between the borrow and the copy — the safety
/// invariant the inline form relied on is established once here.
fn rstring_to_vec(s: RString) -> Vec<u8> {
    // SAFETY: see item doc.
    unsafe { s.as_slice() }.to_vec()
}

/// Frame the Frame 1 preamble from the Service registry's bind paths.
/// The core envelope's byte layout lives on this side of the boundary,
/// so the registry stays a registry and never holds a wire image.
fn frame_preamble(paths: RArray) -> Result<Vec<u8>, MagnusError> {
    Ok(Preamble {
        paths: paths.to_vec()?,
    }
    .encode())
}

/// Frame the Frame 3 snippet table from the registry's entries — one
/// `[kind, name, body]` triple each, `kind` a Symbol naming the form so
/// the wire's discriminant byte stays here. An off-ladder kind raises
/// rather than defaulting, the same fail-closed posture `from_path`
/// takes on its Symbol options.
fn frame_snippets(ruby: &Ruby, entries: RArray) -> Result<Vec<u8>, MagnusError> {
    let mut frame = Snippets {
        entries: Vec::with_capacity(entries.len()),
    };
    for index in 0..entries.len() as isize {
        let entry: RArray = entries.entry(index)?;
        let kind: Symbol = entry.entry(0)?;
        frame.entries.push(match kind.name()?.as_ref() {
            "source" => Snippet::Source {
                name: entry.entry(1)?,
                body: entry.entry(2)?,
            },
            "bytecode" => Snippet::Bytecode {
                body: rstring_to_vec(entry.entry(2)?),
            },
            other => {
                return Err(MagnusError::new(
                    ruby.exception_arg_error(),
                    format!("snippet kind must be :source or :bytecode, got :{other}"),
                ))
            }
        });
    }
    Ok(frame.encode())
}

// ---------------------------------------------------------------------------
// Ruby init
// ---------------------------------------------------------------------------

pub fn init(ruby: &Ruby, kobako: RModule) -> Result<(), MagnusError> {
    // Error hierarchy lives in `lib/kobako/errors.rb`; the ext raises
    // directly into those classes through the constructors and mappers
    // in `runtime/errors.rs` — no intermediate hierarchy is registered.

    let runtime = kobako.define_class("Runtime", ruby.class_object())?;
    runtime.define_singleton_method("from_path", function!(Runtime::from_path, 7))?;
    runtime.define_method("eval", method!(Runtime::eval, 4))?;
    runtime.define_method("run", method!(Runtime::run, 4))?;
    runtime.define_method("profile", method!(Runtime::profile, 0))?;
    // The guest re-enters for a block yield through a frame-scoped
    // `Kobako::Runtime::GuestYielder` the dispatcher hands the Proc, not a
    // method on Runtime.
    bridge::register(runtime)?;

    // Snapshot — the per-invocation result object each entry point returns.
    let snapshot = runtime.define_class("Snapshot", ruby.class_object())?;
    snapshot.define_method("outcome", method!(Snapshot::outcome, 0))?;
    snapshot.define_method("trapped?", method!(Snapshot::trapped, 0))?;
    snapshot.define_method("trap_kind", method!(Snapshot::trap_kind, 0))?;
    snapshot.define_method("trap_message", method!(Snapshot::trap_message, 0))?;
    snapshot.define_method("wall_time", method!(Snapshot::wall_time, 0))?;
    snapshot.define_method("memory_peak", method!(Snapshot::memory_peak, 0))?;
    snapshot.define_method("stdout", method!(Snapshot::stdout, 0))?;
    snapshot.define_method("stdout_truncated?", method!(Snapshot::stdout_truncated, 0))?;
    snapshot.define_method("stderr", method!(Snapshot::stderr, 0))?;
    snapshot.define_method("stderr_truncated?", method!(Snapshot::stderr_truncated, 0))?;

    Ok(())
}

#[derive(TypedData)]
#[magnus(class = "Kobako::Runtime", free_immediately, size)]
struct Runtime {
    // The magnus-free wasmtime driver that runs every invocation; the
    // shell only shuttles Ruby values across its boundary. The Runtime
    // holds no per-invocation state — each `#eval` / `#run` takes its
    // dispatch handler as an argument and returns its whole result as a
    // `Snapshot` — so `Driver`'s own `Send + Sync` carries the type with no
    // interior mutability to guard.
    driver: Driver,
    // Whether each invocation releases Ruby's GVL for its guest span
    // (`gvl: :release`) or holds it throughout (`gvl: :hold`). Fixed at
    // construction; a `bool` carries no interior mutability, so the type
    // stays `Send + Sync`.
    release_gvl: bool,
}

impl DataTypeFunctions for Runtime {}

impl Runtime {
    /// Construct a Runtime from a wasm file path, using the process-wide
    /// shared Engine and per-path Module / InstancePre caches. The single
    /// Ruby-facing constructor for `Kobako::Runtime` — Engine and Module
    /// are never visible to Ruby.
    ///
    /// `timeout_seconds` is the wall-clock cap in seconds
    /// (`None` disables); `memory_limit` is the linear-memory cap in
    /// bytes (`None` disables); `stdout_limit_bytes` / `stderr_limit_bytes`
    /// are the per-channel output caps (`None`
    /// disables); `profile` is the isolation rung the driver builds
    /// (`:permissive` / `:hermetic`); `gvl` is the scheduling mode
    /// (`:hold` / `:release`) deciding whether each invocation releases
    /// the GVL for its guest span. All six are validated by the caller
    /// (`Kobako::Sandbox`); this method only refuses non-finite or
    /// non-positive timeouts, off-ladder profiles, and unrecognized gvl
    /// modes as a defence in depth.
    fn from_path(
        path: String,
        timeout_seconds: Option<f64>,
        memory_limit: Option<usize>,
        stdout_limit_bytes: Option<usize>,
        stderr_limit_bytes: Option<usize>,
        profile: Symbol,
        gvl: Symbol,
    ) -> Result<Self, MagnusError> {
        let ruby = Ruby::get().expect("Ruby thread");
        let timeout = match timeout_seconds {
            None => None,
            Some(secs) if secs.is_finite() && secs > 0.0 => Some(Duration::from_secs_f64(secs)),
            Some(secs) => {
                // An invalid cap argument is a Host App
                // programming error and raises `ArgumentError`, outside the
                // construction-failure `SetupError` branch. `SandboxOptions`
                // is the primary guard (it never lets a bad timeout reach
                // here); this is defence-in-depth for direct `from_path` calls.
                return Err(MagnusError::new(
                    ruby.exception_arg_error(),
                    format!("timeout must be > 0 and finite, got {secs} seconds"),
                ));
            }
        };
        // Fail closed on an off-ladder rung: an unrecognized posture must
        // never fall back to a grant. Same defence-in-depth posture as the
        // timeout guard above — `SandboxOptions` is the primary validator.
        let profile = match profile.name()?.as_ref() {
            "hermetic" => Profile::Hermetic,
            "permissive" => Profile::Permissive,
            other => {
                return Err(MagnusError::new(
                    ruby.exception_arg_error(),
                    format!("profile must be :permissive or :hermetic, got :{other}"),
                ));
            }
        };
        // Same fail-closed posture as `profile`: an unrecognized mode raises
        // rather than defaulting. `SandboxOptions` is the primary validator;
        // this guards direct `from_path` calls.
        let release_gvl = match gvl.name()?.as_ref() {
            "hold" => false,
            "release" => true,
            other => {
                return Err(MagnusError::new(
                    ruby.exception_arg_error(),
                    format!("gvl must be :hold or :release, got :{other}"),
                ));
            }
        };

        let driver = Driver::new(
            Path::new(&path),
            memory_limit,
            Config {
                timeout,
                stdout_limit_bytes,
                stderr_limit_bytes,
                profile,
            },
        )
        .map_err(|e| errors::setup_to_magnus(&ruby, e))?;
        Ok(Self {
            driver,
            release_gvl,
        })
    }

    // -----------------------------------------------------------------
    // Run-path methods. Each takes the run's dispatch handler as its first
    // argument and returns a `Snapshot` for any completed invocation —
    // success or trap alike. Only a could-not-start fault (a missing export
    // or a fault before the export call) raises a `Kobako::TrapError`
    // directly, since it yields no `Snapshot`.
    // -----------------------------------------------------------------

    /// One-shot mruby source execution (`#eval`). Builds the dispatch
    /// handler from `dispatch` (the per-invocation Proc), frames the two
    /// stdin invocation frames from the registry state (`paths`,
    /// `snippets`), hands them and the source to the driver, and returns
    /// the run's `Snapshot`.
    fn eval(
        &self,
        dispatch: Value,
        paths: RArray,
        source: RString,
        snippets: RArray,
    ) -> Result<Snapshot, MagnusError> {
        let ruby = Ruby::get().expect("Ruby thread");
        let handler = build_handler(dispatch);
        let preamble = frame_preamble(paths)?;
        let source = rstring_to_vec(source);
        let snippets = frame_snippets(&ruby, snippets)?;
        // Release the GVL around the guest span iff this Sandbox asks for it;
        // the closure touches no Ruby VALUE (the driver is magnus-free, and a
        // guest→host dispatch re-acquires the GVL through the bridge).
        let result = gvl::region(self.release_gvl, || {
            self.driver.invoke(
                Entry::Eval { source: &source },
                Frames {
                    preamble: &preamble,
                    snippets: &snippets,
                },
                handler,
            )
        });
        let snapshot = result.map_err(|e| errors::to_magnus(&ruby, e))?;
        Ok(Snapshot::from(snapshot))
    }

    /// Execute one entrypoint dispatch (`__kobako_run`) and return its
    /// `Snapshot`.
    ///
    /// The two-frame stdin protocol (preamble + snippets; no user source
    /// frame — docs/wire-codec.md § Invocation channels) plus the
    /// `envelope` copied into guest linear memory; cap semantics match
    /// `#eval`.
    fn run(
        &self,
        dispatch: Value,
        paths: RArray,
        snippets: RArray,
        envelope: RString,
    ) -> Result<Snapshot, MagnusError> {
        let ruby = Ruby::get().expect("Ruby thread");
        let handler = build_handler(dispatch);
        let preamble = frame_preamble(paths)?;
        let snippets = frame_snippets(&ruby, snippets)?;
        let envelope = rstring_to_vec(envelope);
        // Release the GVL around the guest span iff this Sandbox asks for it;
        // see the note in `#eval`.
        let result = gvl::region(self.release_gvl, || {
            self.driver.invoke(
                Entry::Run {
                    envelope: &envelope,
                },
                Frames {
                    preamble: &preamble,
                    snippets: &snippets,
                },
                handler,
            )
        });
        let snapshot = result.map_err(|e| errors::to_magnus(&ruby, e))?;
        Ok(Snapshot::from(snapshot))
    }

    /// Return the isolation profile the driver built, as a Symbol
    /// (`:hermetic` / `:permissive`) — the declaration the Sandbox
    /// compares against the posture its `profile:` option requested.
    fn profile(&self) -> Symbol {
        let ruby = Ruby::get().expect("Ruby thread");
        match self.driver.profile() {
            Profile::Hermetic => ruby.to_symbol("hermetic"),
            Profile::Permissive => ruby.to_symbol("permissive"),
        }
    }
}

/// Build the dispatch handler for one invocation from the per-call `dispatch`
/// Proc. A `nil` Proc yields no handler. The Proc stays GC-rooted for the
/// duration of the synchronous `#eval` / `#run` call as a live method
/// argument on the Ruby stack, so the driver only borrows it (the safety
/// contract on `kobako_runtime::runtime::Runtime`).
fn build_handler(dispatch: Value) -> Option<Arc<dyn DispatchHandler>> {
    if dispatch.is_nil() {
        return None;
    }
    Some(
        Arc::new(bridge::RubyDispatchHandler::new(Opaque::from(dispatch)))
            as Arc<dyn DispatchHandler>,
    )
}

/// One invocation's result at the Ruby boundary — the whole `Snapshot` the
/// driver produced, exposed as `Kobako::Runtime::Snapshot`. Usage and the
/// two output captures are present on every outcome, so the trap path
/// carries them just like the value path; the completion is read as either
/// the outcome bytes (`#outcome`) or a trap (`#trapped?` / `#trap_kind` /
/// `#trap_message`), and the Sandbox layer maps a trap onto its
/// `Kobako::TrapError` family.
#[derive(TypedData)]
#[magnus(class = "Kobako::Runtime::Snapshot", free_immediately, size)]
struct Snapshot {
    completion: Completion,
    stdout: Capture,
    stderr: Capture,
    usage: Usage,
}

impl DataTypeFunctions for Snapshot {}

impl From<RuntimeSnapshot> for Snapshot {
    fn from(snapshot: RuntimeSnapshot) -> Self {
        let RuntimeSnapshot {
            completion,
            stdout,
            stderr,
            usage,
        } = snapshot;
        Self {
            completion,
            stdout,
            stderr,
            usage,
        }
    }
}

impl Snapshot {
    /// The guest's raw outcome bytes on a completed run; empty on a trap,
    /// where `#trapped?` is the authoritative discriminator and the bytes
    /// are never read.
    fn outcome(&self) -> RString {
        let ruby = Ruby::get().expect("Ruby thread");
        match &self.completion {
            Completion::Outcome(bytes) => ruby.str_from_slice(bytes),
            Completion::Trap(_) => ruby.str_from_slice(&[]),
        }
    }

    /// `true` iff the invocation completed via an engine trap.
    fn trapped(&self) -> bool {
        matches!(self.completion, Completion::Trap(_))
    }

    /// The trap's neutral kind as a Symbol (`:timeout` / `:memory_limit` /
    /// `:trap`), or `nil` on a completed run. The Sandbox maps this onto the
    /// named `Kobako::TrapError` subclass.
    fn trap_kind(&self) -> Option<Symbol> {
        let ruby = Ruby::get().expect("Ruby thread");
        match &self.completion {
            Completion::Trap(Trap::Timeout(_)) => Some(ruby.to_symbol("timeout")),
            Completion::Trap(Trap::MemoryLimit(_)) => Some(ruby.to_symbol("memory_limit")),
            Completion::Trap(Trap::Other(_)) => Some(ruby.to_symbol("trap")),
            Completion::Outcome(_) => None,
        }
    }

    /// The trap's message, or `nil` on a completed run.
    fn trap_message(&self) -> Option<String> {
        match &self.completion {
            Completion::Trap(Trap::Timeout(msg) | Trap::MemoryLimit(msg) | Trap::Other(msg)) => {
                Some(msg.clone())
            }
            Completion::Outcome(_) => None,
        }
    }

    /// Wall-clock seconds the guest export call spent inside wasmtime.
    fn wall_time(&self) -> f64 {
        self.usage.wall_time
    }

    /// High-water `memory.grow` delta in bytes past the entry-time baseline.
    fn memory_peak(&self) -> usize {
        self.usage.memory_peak
    }

    /// Bytes captured on the guest's stdout channel, clipped to the cap.
    fn stdout(&self) -> RString {
        let ruby = Ruby::get().expect("Ruby thread");
        ruby.str_from_slice(&self.stdout.bytes)
    }

    /// `true` iff the stdout channel reached its cap during this run.
    fn stdout_truncated(&self) -> bool {
        self.stdout.truncated
    }

    /// Bytes captured on the guest's stderr channel, clipped to the cap.
    fn stderr(&self) -> RString {
        let ruby = Ruby::get().expect("Ruby thread");
        ruby.str_from_slice(&self.stderr.bytes)
    }

    /// `true` iff the stderr channel reached its cap during this run.
    fn stderr_truncated(&self) -> bool {
        self.stderr.truncated
    }
}
