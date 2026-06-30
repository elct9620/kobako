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

use crate::contract::error::{Error, SetupError, Trap};

/// Resolve `Kobako::<name>` as an `ExceptionClass` — the shared body of
/// every error-class `Lazy` below, which differ only in the constant
/// name. The constants are guaranteed present by the time any of these
/// lazies first resolve (`lib/kobako/errors.rb` loads the hierarchy before
/// the ext raises into it), so a missing constant is a build / wiring bug
/// and the `unwrap` is the correct fail-fast.
fn kobako_error_class(ruby: &Ruby, name: &str) -> ExceptionClass {
    let kobako: RModule = ruby.class_object().const_get("Kobako").unwrap();
    kobako.const_get(name).unwrap()
}

pub(crate) static SETUP_ERROR: Lazy<ExceptionClass> =
    Lazy::new(|ruby| kobako_error_class(ruby, "SetupError"));

pub(crate) static MODULE_NOT_BUILT_ERROR: Lazy<ExceptionClass> =
    Lazy::new(|ruby| kobako_error_class(ruby, "ModuleNotBuiltError"));

pub(crate) static TRAP_ERROR: Lazy<ExceptionClass> =
    Lazy::new(|ruby| kobako_error_class(ruby, "TrapError"));

pub(crate) static TIMEOUT_ERROR: Lazy<ExceptionClass> =
    Lazy::new(|ruby| kobako_error_class(ruby, "TimeoutError"));

pub(crate) static MEMORY_LIMIT_ERROR: Lazy<ExceptionClass> =
    Lazy::new(|ruby| kobako_error_class(ruby, "MemoryLimitError"));

pub(crate) static SANDBOX_ERROR: Lazy<ExceptionClass> =
    Lazy::new(|ruby| kobako_error_class(ruby, "SandboxError"));

/// Build a `MagnusError` in `class` carrying `msg` — the shared body of
/// the named `*_err` constructors below, which differ only in which
/// error-class `Lazy` they target.
fn error_in(ruby: &Ruby, class: &Lazy<ExceptionClass>, msg: impl Into<String>) -> MagnusError {
    MagnusError::new(ruby.get_inner(class), msg.into())
}

/// Construct a `Kobako::TrapError` magnus error. Used for every
/// invocation-time wasmtime engine failure that is not a configured-cap
/// trap — missing exports, allocation faults, memory write/read failures.
/// Construction-time setup failures use `setup_err`, not this.
pub(crate) fn trap_err(ruby: &Ruby, msg: impl Into<String>) -> MagnusError {
    error_in(ruby, &TRAP_ERROR, msg)
}

/// Construct a `Kobako::SetupError` magnus error. Used for every
/// construction-time failure on the `Runtime.from_path` path before any
/// invocation runs — unreadable artifact, bytes that are not a valid Wasm
/// module, or engine / linker / instantiation setup failure. The
/// `ModuleNotBuiltError` subclass (artifact absent) is
/// raised through `MODULE_NOT_BUILT_ERROR` directly.
pub(crate) fn setup_err(ruby: &Ruby, msg: impl Into<String>) -> MagnusError {
    error_in(ruby, &SETUP_ERROR, msg)
}

/// Construct a `Kobako::TimeoutError` magnus error. Surfaces the
/// wall-clock cap path with the verb prefix added
/// by `Kobako::Sandbox#invoke!`.
fn timeout_err(ruby: &Ruby, msg: impl Into<String>) -> MagnusError {
    error_in(ruby, &TIMEOUT_ERROR, msg)
}

/// Construct a `Kobako::MemoryLimitError` magnus error. Surfaces the
/// linear-memory cap path with the verb prefix
/// added by `Kobako::Sandbox#invoke!`.
fn memory_limit_err(ruby: &Ruby, msg: impl Into<String>) -> MagnusError {
    error_in(ruby, &MEMORY_LIMIT_ERROR, msg)
}

/// Construct a `Kobako::SandboxError` magnus error. Used for the
/// host-side pre-call faults the SPEC attributes to the sandbox / wire
/// layer rather than the Wasm engine — currently the `#run` invocation
/// envelope reservation failure (`__kobako_alloc` returns 0).
/// The runtime is intact, so this must not be a
/// `TrapError`: no discard-and-recreate recovery is owed to the caller.
fn sandbox_err(ruby: &Ruby, msg: impl Into<String>) -> MagnusError {
    error_in(ruby, &SANDBOX_ERROR, msg)
}

/// Map a neutral `Trap` onto its `Kobako::TrapError`-family Ruby exception.
/// The boundary between the magnus-free run mechanics and the Ruby surface:
/// the run path classifies a fault into a `Trap`, and this is where it
/// becomes a raised exception.
pub(crate) fn trap_to_magnus(ruby: &Ruby, trap: Trap) -> MagnusError {
    match trap {
        Trap::Timeout(msg) => timeout_err(ruby, msg),
        Trap::MemoryLimit(msg) => memory_limit_err(ruby, msg),
        Trap::Other(msg) => trap_err(ruby, msg),
    }
}

/// Map a neutral `SetupError` onto the `Kobako::*` class the SPEC assigns
/// to each runtime state — artifact-absent, runtime-dead, runtime-intact.
pub(crate) fn setup_to_magnus(ruby: &Ruby, err: SetupError) -> MagnusError {
    match err {
        SetupError::ModuleNotBuilt(msg) => error_in(ruby, &MODULE_NOT_BUILT_ERROR, msg),
        SetupError::Dead(msg) => setup_err(ruby, msg),
        SetupError::Intact(msg) => sandbox_err(ruby, msg),
    }
}

/// Map either run-path channel onto its Ruby exception. The single
/// translation point the run-path entry methods funnel their `Result`
/// through.
pub(crate) fn to_magnus(ruby: &Ruby, err: Error) -> MagnusError {
    match err {
        Error::Trap(trap) => trap_to_magnus(ruby, trap),
        Error::Setup(err) => setup_to_magnus(ruby, err),
    }
}
