//! Per-invocation wasmtime export handles for the host-driven ABI
//! surface.
//!
//! Each ABI export is named and typed once, here, against each fresh
//! per-invocation instance. The invocation body and the host imports
//! running inside it both drive the guest through these handles, never
//! by looking an export up by name. Distinct from `crate::cache` (the
//! process-wide Engine / Module cache): this carries *which guest
//! function to call*, per invocation.

use wasmtime::{AsContextMut, Instance as WtInstance, Memory, TypedFunc};

use kobako_runtime::endpoint::RUNTIME_INCOMPATIBLE;
use kobako_runtime::error::Trap;

/// The resolved host-driven export handles. Each is `Option` because test
/// fixtures (a minimal "ping" module) need not provide them; real
/// `kobako.wasm` always does, and the run path surfaces a `Trap` when a
/// handle is `None`.
///
/// The handles are indices into the owning Store, not borrows of the
/// `Instance` — they stay valid for the Store's lifetime, which is why
/// no `Instance` field is kept, and why a clone can ride in the Store's
/// own data for the host imports to reach.
#[derive(Clone)]
pub(crate) struct Exports {
    pub(crate) downcall: Option<TypedFunc<(i32, i32), u64>>,
    pub(crate) alloc: Option<TypedFunc<u32, u32>>,
    pub(crate) memory: Option<Memory>,
}

impl Exports {
    /// A missing export is not an error here, because the test fixture is a
    /// bare module; presence is enforced at invocation time.
    pub(crate) fn resolve(instance: &WtInstance, mut ctx: impl AsContextMut) -> Self {
        Self {
            downcall: instance
                .get_typed_func::<(i32, i32), u64>(&mut ctx, "__kobako_downcall")
                .ok(),
            alloc: instance
                .get_typed_func::<u32, u32>(&mut ctx, "__kobako_alloc")
                .ok(),
            memory: instance.get_memory(&mut ctx, "memory"),
        }
    }
}

pub(crate) fn require<Params, Results>(
    export: Option<&TypedFunc<Params, Results>>,
) -> Result<&TypedFunc<Params, Results>, Trap>
where
    Params: wasmtime::WasmParams,
    Results: wasmtime::WasmResults,
{
    export.ok_or_else(|| Trap::Other(RUNTIME_INCOMPATIBLE.to_string()))
}

#[cfg(test)]
mod tests {
    //! The ABI is a closed set of guest exports, so the bundled Guest
    //! Binary is read for what it exports rather than only for whether
    //! `resolve` finds what it looks up — an extra entry point would pass
    //! every lookup. A missing binary is a hard failure under
    //! CI (which always builds it) and a silent skip locally.
    use std::path::Path;

    use wasmtime::ExternType;

    use crate::cache::cached_module;

    const WASM: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/kobako.wasm");

    const ABI_EXPORTS: [&str; 3] = [
        "__kobako_abi_version",
        "__kobako_alloc",
        "__kobako_downcall",
    ];

    // @behavior RT-068
    #[test]
    fn the_guest_binary_exports_the_abi_functions_and_nothing_else() {
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
            "data/kobako.wasm read through its export section must export exactly the ABI's functions"
        );
    }
}
