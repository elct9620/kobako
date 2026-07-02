//! Per-invocation wasmtime export handles for the host-driven ABI
//! surface.
//!
//! `Driver::instantiate` resolves the ABI exports the run path drives
//! (`__kobako_eval` / `__kobako_run` / `__kobako_take_outcome` /
//! `__kobako_alloc`) plus the `memory` export against each fresh
//! per-invocation instance and bundles their
//! typed handles here, so the invocation body passes one struct around
//! rather than re-resolving exports by name at every step. Distinct
//! from `crate::cache` (the process-wide Engine / Module cache): this
//! carries *which guest function to call*, per invocation.
//!
//! `crate::dispatch` does not reach this struct — a host import runs
//! against a `Caller`, so the dispatch path resolves `__kobako_alloc`
//! and `memory` through `Caller::get_export` instead.

use wasmtime::{AsContextMut, Instance as WtInstance, Memory, TypedFunc};

/// The resolved host-driven export handles. Each is `Option` because test
/// fixtures (a minimal "ping" module) need not provide them; real
/// `kobako.wasm` always does, and the run-path methods surface a `Trap`
/// (via `require_export` / `require_memory`) when a handle is `None`.
///
/// The handles are indices into the owning Store, not borrows of the
/// `Instance` — they stay valid for the Store's lifetime, which is why
/// no `Instance` field is kept.
pub(crate) struct Exports {
    pub(crate) eval: Option<TypedFunc<(), ()>>,
    pub(crate) run: Option<TypedFunc<(i32, i32), ()>>,
    pub(crate) take_outcome: Option<TypedFunc<(), u64>>,
    pub(crate) alloc: Option<TypedFunc<u32, u32>>,
    pub(crate) memory: Option<Memory>,
}

impl Exports {
    /// Best-effort lookup of the host-driven exports against a freshly
    /// instantiated module. Missing exports are not an error here
    /// (the test fixture is a bare module); the host enforces presence at
    /// invocation time. Only the SPEC ABI shapes are accepted —
    /// `__kobako_eval` is `() -> ()`, `__kobako_run` is
    /// `(env_ptr, env_len) -> ()`, `__kobako_take_outcome` is `() -> u64`,
    /// `__kobako_alloc` is `(len) -> ptr`
    /// (docs/wire-codec.md § ABI Signatures).
    pub(crate) fn resolve(instance: &WtInstance, mut ctx: impl AsContextMut) -> Self {
        Self {
            eval: instance
                .get_typed_func::<(), ()>(&mut ctx, "__kobako_eval")
                .ok(),
            run: instance
                .get_typed_func::<(i32, i32), ()>(&mut ctx, "__kobako_run")
                .ok(),
            take_outcome: instance
                .get_typed_func::<(), u64>(&mut ctx, "__kobako_take_outcome")
                .ok(),
            alloc: instance
                .get_typed_func::<u32, u32>(&mut ctx, "__kobako_alloc")
                .ok(),
            memory: instance.get_memory(&mut ctx, "memory"),
        }
    }
}
