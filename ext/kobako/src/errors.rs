//! Ruby error classes (lazy-resolved once the top-level Kobako error
//! hierarchy is loaded by `lib/kobako/errors.rb`), the conversions into
//! them every Context module shares, and the reading of a Ruby `Errno`
//! class back into the runtime's `Errno`. The ext raises
//! directly into the invocation-outcome taxonomy (`TrapError` and its
//! subclasses) for run-path failures and into the construction-layer
//! `SetupError` (and its `GuestBinaryNotFoundError` subclass) for `from_path`
//! setup failures — no engine-specific intermediate layer; the Sandbox
//! layer adds the verb prefix and lets the subclass identity flow through
//! unchanged.

use magnus::value::Lazy;
use magnus::{prelude::*, Error as MagnusError, ExceptionClass, RClass, Ruby};

use kobako_runtime::error::{InvokeError, SetupError, Trap};
use kobako_runtime::Errno;

static SETUP_ERROR: Lazy<ExceptionClass> =
    Lazy::new(|ruby| crate::constant_by_path(ruby, &["SetupError"]));

static MODULE_NOT_BUILT_ERROR: Lazy<ExceptionClass> =
    Lazy::new(|ruby| crate::constant_by_path(ruby, &["GuestBinaryNotFoundError"]));

static TRAP_ERROR: Lazy<ExceptionClass> =
    Lazy::new(|ruby| crate::constant_by_path(ruby, &["TrapError"]));

static TIMEOUT_ERROR: Lazy<ExceptionClass> =
    Lazy::new(|ruby| crate::constant_by_path(ruby, &["TimeoutError"]));

static MEMORY_LIMIT_ERROR: Lazy<ExceptionClass> =
    Lazy::new(|ruby| crate::constant_by_path(ruby, &["MemoryLimitError"]));

static SANDBOX_ERROR: Lazy<ExceptionClass> =
    Lazy::new(|ruby| crate::constant_by_path(ruby, &["SandboxError"]));

fn error_in(ruby: &Ruby, class: &Lazy<ExceptionClass>, msg: impl Into<String>) -> MagnusError {
    MagnusError::new(ruby.get_inner(class), msg.into())
}

/// For an invocation-time engine failure that is not a configured cap;
/// construction-time failures use `from_setup`.
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

/// The verb prefix is left to `Kobako::Context#invoke`.
pub(super) fn from_trap(ruby: &Ruby, trap: Trap) -> MagnusError {
    MagnusError::new(trap_class(ruby, &trap), trap.to_string())
}

pub(super) fn from_setup(ruby: &Ruby, err: SetupError) -> MagnusError {
    match err {
        // The engine serves every frontend, so the gem adds its own way to
        // build the artifact.
        SetupError::GuestBinaryNotFound(msg) => error_in(
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

pub(super) fn from_invoke(ruby: &Ruby, err: InvokeError) -> MagnusError {
    match err {
        InvokeError::Trap(trap) => from_trap(ruby, trap),
        InvokeError::Setup(err) => from_setup(ruby, err),
        InvokeError::Entry(err) => MagnusError::new(ruby.exception_arg_error(), err.to_string()),
        // A failure this frontend does not know is still a run that never
        // started, which the taxonomy attributes to the engine.
        other => trap_err(ruby, other.to_string()),
    }
}

/// The failure the nearest `Errno::` class among `class`'s ancestors names
/// by its POSIX name, so an `Errno::ENOENT` and its subclasses read alike;
/// `None` for a class no `Errno::` class is behind.
pub(super) fn errno_of(class: RClass) -> Option<Errno> {
    class.ancestors().into_iter().find_map(|module| {
        let name: Option<String> = module.funcall("name", ()).ok()?;
        Errno::from_posix_name(name?.strip_prefix("Errno::")?)
    })
}
