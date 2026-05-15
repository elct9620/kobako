// Host-side wasmtime wrapper.
//
// The only Ruby-visible class is
//
//   Kobako::Wasm::Instance — wraps wasmtime::Instance + cached TypedFuncs
//
// constructed via `Kobako::Wasm::Instance.from_path(path, timeout, memory_limit,
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
//   * `instance`    — Kobako::Wasm::Instance and its run-path methods.
//   * `dispatch`    — `__kobako_dispatch` host-import dispatch helpers.
//
// This file is the façade: it owns the Ruby error class lazy-resolvers,
// the `wasm_err` / `timeout_err` / `memory_limit_err` constructors shared
// by every submodule, and the Ruby init() that registers
// `Kobako::Wasm::Instance` and its methods.

mod cache;
mod dispatch;
mod host_state;
mod instance;

use magnus::value::Lazy;
use magnus::{function, method, prelude::*, Error as MagnusError, ExceptionClass, RModule, Ruby};

use instance::Instance;

// ---------------------------------------------------------------------------
// Error classes (lazy-resolved from Ruby once Kobako::Wasm is defined).
// ---------------------------------------------------------------------------

pub(crate) static MODULE_NOT_BUILT_ERROR: Lazy<ExceptionClass> = Lazy::new(|ruby| {
    let kobako: RModule = ruby.class_object().const_get("Kobako").unwrap();
    let wasm: RModule = kobako.const_get("Wasm").unwrap();
    wasm.const_get("ModuleNotBuiltError").unwrap()
});

pub(crate) static WASM_ERROR: Lazy<ExceptionClass> = Lazy::new(|ruby| {
    let kobako: RModule = ruby.class_object().const_get("Kobako").unwrap();
    let wasm: RModule = kobako.const_get("Wasm").unwrap();
    wasm.const_get("Error").unwrap()
});

pub(crate) static WASM_TIMEOUT_ERROR: Lazy<ExceptionClass> = Lazy::new(|ruby| {
    let kobako: RModule = ruby.class_object().const_get("Kobako").unwrap();
    let wasm: RModule = kobako.const_get("Wasm").unwrap();
    wasm.const_get("TimeoutError").unwrap()
});

pub(crate) static WASM_MEMORY_LIMIT_ERROR: Lazy<ExceptionClass> = Lazy::new(|ruby| {
    let kobako: RModule = ruby.class_object().const_get("Kobako").unwrap();
    let wasm: RModule = kobako.const_get("Wasm").unwrap();
    wasm.const_get("MemoryLimitError").unwrap()
});

pub(crate) fn wasm_err(ruby: &Ruby, msg: impl Into<String>) -> MagnusError {
    MagnusError::new(ruby.get_inner(&WASM_ERROR), msg.into())
}

/// Construct a `Kobako::Wasm::TimeoutError` magnus error. Surfaces the
/// SPEC.md E-19 wall-clock cap path so the Sandbox layer can rewrap it
/// as `Kobako::TimeoutError`.
pub(crate) fn timeout_err(ruby: &Ruby, msg: impl Into<String>) -> MagnusError {
    MagnusError::new(ruby.get_inner(&WASM_TIMEOUT_ERROR), msg.into())
}

/// Construct a `Kobako::Wasm::MemoryLimitError` magnus error. Surfaces
/// the SPEC.md E-20 linear-memory cap path so the Sandbox layer can
/// rewrap it as `Kobako::MemoryLimitError`.
pub(crate) fn memory_limit_err(ruby: &Ruby, msg: impl Into<String>) -> MagnusError {
    MagnusError::new(ruby.get_inner(&WASM_MEMORY_LIMIT_ERROR), msg.into())
}

// ---------------------------------------------------------------------------
// Ruby init
// ---------------------------------------------------------------------------

pub fn init(ruby: &Ruby, kobako: RModule) -> Result<(), MagnusError> {
    let wasm = kobako.define_module("Wasm")?;

    // Error hierarchy. ModuleNotBuiltError is the headline error for the
    // common pre-build state where `data/kobako.wasm` has not yet been
    // produced (e.g. fresh clone before `rake compile`). TimeoutError and
    // MemoryLimitError carry the SPEC.md B-01 per-run cap paths up to the
    // Sandbox layer.
    let base_err = wasm.define_error("Error", ruby.exception_standard_error())?;
    wasm.define_error("ModuleNotBuiltError", base_err)?;
    wasm.define_error("TimeoutError", base_err)?;
    wasm.define_error("MemoryLimitError", base_err)?;

    let instance = wasm.define_class("Instance", ruby.class_object())?;
    instance.define_singleton_method("from_path", function!(Instance::from_path, 5))?;
    instance.define_method("server=", method!(Instance::set_server, 1))?;
    instance.define_method("run", method!(Instance::run, 2))?;
    instance.define_method("stdout", method!(Instance::stdout, 0))?;
    instance.define_method("stderr", method!(Instance::stderr, 0))?;
    instance.define_method("outcome!", method!(Instance::outcome, 0))?;

    Ok(())
}
