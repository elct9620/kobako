//! Cached wasmtime export handles for the host-driven ABI surface.
//!
//! `Runtime::from_path` resolves the three docs/wire-codec.md ABI exports
//! the run path drives (`__kobako_eval` / `__kobako_run` /
//! `__kobako_take_outcome`) once at construction and stores their typed
//! handles here, so each `#eval` / `#run` calls a cached handle rather than
//! re-resolving the export by name. Distinct from `super::cache` (the
//! process-wide Engine / Module cache): this caches *which guest function
//! to call*, per `Runtime`.
//!
//! `__kobako_alloc` is deliberately absent — only `super::dispatch` calls
//! it, and it does so through `Caller::get_export` on the wasmtime side.

use wasmtime::{AsContextMut, Instance as WtInstance, TypedFunc};

use super::invocation::StoreCell;

/// The cached host-driven export handles. Each is `Option` because test
/// fixtures (a minimal "ping" module) need not provide them; real
/// `kobako.wasm` always does, and the run-path methods raise a Ruby
/// `Kobako::TrapError` (via `require_export`) when a handle is `None`.
pub(crate) struct Exports {
    pub(crate) eval: Option<TypedFunc<(), ()>>,
    pub(crate) run: Option<TypedFunc<(i32, i32), ()>>,
    pub(crate) take_outcome: Option<TypedFunc<(), u64>>,
}

impl Exports {
    /// Best-effort lookup of the three host-driven exports against a
    /// freshly instantiated module. Missing exports are not an error here
    /// (the test fixture is a bare module); the host enforces presence at
    /// invocation time. Only the SPEC ABI shapes are accepted —
    /// `__kobako_eval` is `() -> ()`, `__kobako_run` is
    /// `(env_ptr, env_len) -> ()`, `__kobako_take_outcome` is `() -> u64`
    /// (docs/wire-codec.md § ABI Signatures).
    pub(crate) fn resolve(instance: &WtInstance, store: &StoreCell) -> Self {
        let mut store_ref = store.borrow_mut();
        let mut ctx = store_ref.as_context_mut();
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
        }
    }
}
