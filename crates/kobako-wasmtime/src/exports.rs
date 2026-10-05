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
    /// invocation time. Only the ABI shapes are accepted —
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

#[cfg(test)]
mod tests {
    //! The invocation ABI is a closed set of guest exports, so the bundled
    //! Guest Binary is read for what it exports rather than only for
    //! whether `resolve` finds what it looks up — an extra entry point
    //! would pass every lookup. A missing binary is a hard failure under
    //! CI (which always builds it) and a silent skip locally.
    use std::path::Path;

    use wasmtime::ExternType;

    use crate::cache::cached_module;

    const WASM: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/kobako.wasm");

    const ABI_EXPORTS: [&str; 6] = [
        "__kobako_abi_version",
        "__kobako_alloc",
        "__kobako_eval",
        "__kobako_run",
        "__kobako_take_outcome",
        "__kobako_yield_to_block",
    ];

    // @behavior RT-068
    #[test]
    fn the_guest_binary_exports_the_six_abi_functions_and_nothing_else() {
        let path = Path::new(WASM);
        if !path.exists() {
            assert!(
                std::env::var_os("CI").is_none(),
                "data/kobako.wasm missing under CI — run `bundle exec rake wasm:build`"
            );
            return;
        }
        let module = cached_module(path).expect("the Guest Binary must load");

        let mut functions: Vec<&str> = module
            .exports()
            .filter(|export| matches!(export.ty(), ExternType::Func(_)))
            .map(|export| export.name())
            .collect();
        functions.sort_unstable();

        assert_eq!(
            functions, ABI_EXPORTS,
            "data/kobako.wasm read through its export section must export exactly the six ABI functions"
        );
    }
}
