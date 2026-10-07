//! Ruby error classes (lazy-resolved once the top-level Kobako error
//! hierarchy is loaded by `lib/kobako/errors.rb`) and the `*_err`
//! constructors every run-mechanics submodule shares. The ext raises
//! directly into the invocation-outcome taxonomy (`TrapError` and its
//! subclasses) for run-path failures and into the construction-layer
//! `SetupError` (and its `ModuleNotBuiltError` subclass) for `from_path`
//! setup failures — no engine-specific intermediate layer; the Sandbox
//! layer adds the verb prefix and lets the subclass identity flow through
//! unchanged.

use magnus::value::Lazy;
use magnus::{prelude::*, Error as MagnusError, ExceptionClass, RModule, Ruby};

use kobako_runtime::error::{InvokeError, SetupError, Trap};

/// `lib/kobako/errors.rb` loads the hierarchy before the ext raises into
/// it, so a missing constant is a wiring bug and the `unwrap` fails fast.
fn kobako_error_class(ruby: &Ruby, name: &str) -> ExceptionClass {
    let kobako: RModule = ruby.class_object().const_get("Kobako").unwrap();
    kobako.const_get(name).unwrap()
}

static SETUP_ERROR: Lazy<ExceptionClass> = Lazy::new(|ruby| kobako_error_class(ruby, "SetupError"));

static MODULE_NOT_BUILT_ERROR: Lazy<ExceptionClass> =
    Lazy::new(|ruby| kobako_error_class(ruby, "ModuleNotBuiltError"));

static TRAP_ERROR: Lazy<ExceptionClass> = Lazy::new(|ruby| kobako_error_class(ruby, "TrapError"));

static TIMEOUT_ERROR: Lazy<ExceptionClass> =
    Lazy::new(|ruby| kobako_error_class(ruby, "TimeoutError"));

static MEMORY_LIMIT_ERROR: Lazy<ExceptionClass> =
    Lazy::new(|ruby| kobako_error_class(ruby, "MemoryLimitError"));

static SANDBOX_ERROR: Lazy<ExceptionClass> =
    Lazy::new(|ruby| kobako_error_class(ruby, "SandboxError"));

fn error_in(ruby: &Ruby, class: &Lazy<ExceptionClass>, msg: impl Into<String>) -> MagnusError {
    MagnusError::new(ruby.get_inner(class), msg.into())
}

/// For an invocation-time engine failure that is not a configured cap;
/// construction-time failures use `setup_err`.
pub(super) fn trap_err(ruby: &Ruby, msg: impl Into<String>) -> MagnusError {
    error_in(ruby, &TRAP_ERROR, msg)
}

/// The one place a trap meets the `Kobako::TrapError` family, whether it
/// stopped the run before the guest started or while it ran.
pub(super) fn trap_class(ruby: &Ruby, trap: &Trap) -> ExceptionClass {
    match trap {
        Trap::Timeout(_) => ruby.get_inner(&TIMEOUT_ERROR),
        Trap::MemoryLimit(_) => ruby.get_inner(&MEMORY_LIMIT_ERROR),
        // A cap with no named subclass here is still an engine fault, so it
        // takes the base class rather than borrowing another cap's name.
        _ => ruby.get_inner(&TRAP_ERROR),
    }
}

/// The verb prefix is left to `Kobako::Context#invoke!`.
pub(super) fn trap_to_magnus(ruby: &Ruby, trap: Trap) -> MagnusError {
    MagnusError::new(trap_class(ruby, &trap), trap.to_string())
}

pub(super) fn setup_to_magnus(ruby: &Ruby, err: SetupError) -> MagnusError {
    match err {
        // The engine serves every frontend, so the gem adds its own way to
        // build the artifact.
        SetupError::ModuleNotBuilt(msg) => error_in(
            ruby,
            &MODULE_NOT_BUILT_ERROR,
            format!("{msg}; run `bundle exec rake wasm:build` to build it"),
        ),
        SetupError::Dead(msg) => error_in(ruby, &SETUP_ERROR, msg),
        // Runtime intact means a host-side pre-call fault attributed to
        // the sandbox / wire layer: the engine never ran, so never a
        // TrapError.
        SetupError::Intact(msg) => error_in(ruby, &SANDBOX_ERROR, msg),
        // An unrecognised state is one this frontend cannot promise is
        // recoverable, so it takes the same class as `Dead`.
        other => error_in(ruby, &SETUP_ERROR, other.to_string()),
    }
}

pub(super) fn to_magnus(ruby: &Ruby, err: InvokeError) -> MagnusError {
    match err {
        InvokeError::Trap(trap) => trap_to_magnus(ruby, trap),
        InvokeError::Setup(err) => setup_to_magnus(ruby, err),
        // A channel this frontend does not know is still a run that never
        // started, which the taxonomy attributes to the engine.
        other => trap_err(ruby, other.to_string()),
    }
}
