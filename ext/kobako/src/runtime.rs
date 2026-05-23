// Host-side wasmtime runtime wrapper.
//
// The only Ruby-visible class is
//
//   Kobako::Runtime — wraps wasmtime::Instance + cached TypedFuncs
//
// constructed via `Kobako::Runtime.from_path(path, timeout, memory_limit,
// stdout_limit, stderr_limit)`.
// The underlying wasmtime Engine and compiled Module live in a process-scope
// cache (see the `cache` submodule) and never surface to Ruby (SPEC.md "Code
// Organization": `ext/` "exposes no Wasm engine types to the Host App or
// downstream gems").
//
// Module layout (per CLAUDE.md principle #2 — one responsibility per file):
//
//   * `cache`       — process-wide Engine + per-path Module cache and the
//                     process-singleton epoch ticker thread.
//   * `host_state`  — HostState (per-Store context), StoreCell wrapper, the
//                     [`KobakoLimiter`] memory cap, and the trap marker
//                     types ([`TimeoutTrap`] / [`MemoryLimitTrap`]).
//   * `instance`    — `Kobako::Runtime` magnus class and its run-path methods.
//   * `dispatch`    — `__kobako_dispatch` host-import dispatch helpers.
//
// This file is the façade: it owns the Ruby error class lazy-resolvers,
// the `trap_err` / `timeout_err` / `memory_limit_err` constructors shared
// by every submodule, and the Ruby init() that registers `Kobako::Runtime`
// and its methods.

mod cache;
mod dispatch;
mod host_state;
mod instance;

use magnus::value::Lazy;
use magnus::{
    function, method, prelude::*, Error as MagnusError, ExceptionClass, RModule, RString, Ruby,
};

use instance::Instance;

/// Copy the bytes of +s+ into a fresh +Vec<u8>+. Single safe entry to
/// what would otherwise be an inline +unsafe { rstring.as_slice() }
/// .to_vec()+ duplicated at every host-↔-guest boundary. The borrow
/// does not outlive this call, so no Ruby allocation can move the
/// underlying RString between the borrow and the copy — the safety
/// invariant the inline form relied on is established once here.
pub(crate) fn rstring_to_vec(s: RString) -> Vec<u8> {
    // SAFETY: see item doc.
    unsafe { s.as_slice() }.to_vec()
}

// ---------------------------------------------------------------------------
// Error classes (lazy-resolved from Ruby once the top-level Kobako error
// hierarchy is loaded by `lib/kobako/errors.rb`). The ext raises directly
// into the three-class taxonomy — no engine-specific intermediate layer;
// the Sandbox layer adds the verb prefix and lets the subclass identity
// flow through unchanged.
// ---------------------------------------------------------------------------

pub(crate) static MODULE_NOT_BUILT_ERROR: Lazy<ExceptionClass> = Lazy::new(|ruby| {
    let kobako: RModule = ruby.class_object().const_get("Kobako").unwrap();
    kobako.const_get("ModuleNotBuiltError").unwrap()
});

pub(crate) static TRAP_ERROR: Lazy<ExceptionClass> = Lazy::new(|ruby| {
    let kobako: RModule = ruby.class_object().const_get("Kobako").unwrap();
    kobako.const_get("TrapError").unwrap()
});

pub(crate) static TIMEOUT_ERROR: Lazy<ExceptionClass> = Lazy::new(|ruby| {
    let kobako: RModule = ruby.class_object().const_get("Kobako").unwrap();
    kobako.const_get("TimeoutError").unwrap()
});

pub(crate) static MEMORY_LIMIT_ERROR: Lazy<ExceptionClass> = Lazy::new(|ruby| {
    let kobako: RModule = ruby.class_object().const_get("Kobako").unwrap();
    kobako.const_get("MemoryLimitError").unwrap()
});

/// Construct a `Kobako::TrapError` magnus error. Used for every wasmtime
/// engine failure that is not a configured-cap trap — missing exports,
/// allocation faults, instantiation errors, memory write/read failures.
pub(crate) fn trap_err(ruby: &Ruby, msg: impl Into<String>) -> MagnusError {
    MagnusError::new(ruby.get_inner(&TRAP_ERROR), msg.into())
}

/// Construct a `Kobako::TimeoutError` magnus error. Surfaces the
/// docs/behavior.md E-19 wall-clock cap path with the verb prefix added
/// by `Kobako::Sandbox#invoke!`.
pub(crate) fn timeout_err(ruby: &Ruby, msg: impl Into<String>) -> MagnusError {
    MagnusError::new(ruby.get_inner(&TIMEOUT_ERROR), msg.into())
}

/// Construct a `Kobako::MemoryLimitError` magnus error. Surfaces the
/// docs/behavior.md E-20 linear-memory cap path with the verb prefix
/// added by `Kobako::Sandbox#invoke!`.
pub(crate) fn memory_limit_err(ruby: &Ruby, msg: impl Into<String>) -> MagnusError {
    MagnusError::new(ruby.get_inner(&MEMORY_LIMIT_ERROR), msg.into())
}

// ---------------------------------------------------------------------------
// Ruby init
// ---------------------------------------------------------------------------

pub fn init(ruby: &Ruby, kobako: RModule) -> Result<(), MagnusError> {
    // Error hierarchy lives in `lib/kobako/errors.rb` (top-level
    // `Kobako::TrapError` / `TimeoutError` / `MemoryLimitError` /
    // `ModuleNotBuiltError`). The ext raises directly into those classes
    // through `trap_err` / `timeout_err` / `memory_limit_err` /
    // `MODULE_NOT_BUILT_ERROR`; no intermediate hierarchy is registered.

    let runtime = kobako.define_class("Runtime", ruby.class_object())?;
    runtime.define_singleton_method("from_path", function!(Instance::from_path, 5))?;
    runtime.define_method("channel=", method!(Instance::set_channel, 1))?;
    runtime.define_method("yield_to_block", method!(Instance::yield_to_block, 1))?;
    runtime.define_method("eval", method!(Instance::eval, 3))?;
    runtime.define_method("run", method!(Instance::run, 3))?;
    runtime.define_method("usage", method!(Instance::usage, 0))?;

    Ok(())
}
