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

    use kobako_runtime::profile::Profile;
    use wasmtime::{ExternType, Store};

    use crate::cache::{guest_binary_built, load_engine, load_module};
    use crate::config::Config;
    use crate::imports;
    use crate::invocation::Invocation;
    use crate::trap::NO_TIMEOUT_EPOCH_DELTA;
    use crate::wasi::{sandbox_connections, Wasi};

    const WASM: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/kobako.wasm");

    const ABI_EXPORTS: [&str; 4] = [
        "__kobako_abi_version",
        "__kobako_alloc",
        "__kobako_downcall",
        "cabi_realloc",
    ];

    // @behavior RT-068
    #[test]
    fn the_guest_binary_exports_the_abi_functions_and_nothing_else() {
        let path = Path::new(WASM);
        if !guest_binary_built(path.exists(), "data/kobako.wasm") {
            return;
        }
        let module = load_module(path).expect("the Guest Binary must load");

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

    // @behavior RT-375
    #[test]
    fn the_guest_binary_hands_an_empty_list_no_room_and_traps_with_none_left() {
        let path = Path::new(WASM);
        if !guest_binary_built(path.exists(), "data/kobako.wasm") {
            return;
        }
        let module = load_module(path).expect("the Guest Binary must load");
        let config = Config::for_profile(Profile::Hermetic);
        let wasi = Wasi::for_invocation(&config, Vec::new(), &[], &sandbox_connections())
            .expect("WASI context must build");
        let mut store = Store::new(
            load_engine().expect("shared engine must be constructible"),
            Invocation::new(wasi),
        );
        store.set_epoch_deadline(NO_TIMEOUT_EPOCH_DELTA);
        let instance = imports::linker()
            .expect("host imports must link")
            .instantiate(&mut store, &module)
            .expect("the Guest Binary must instantiate");
        let realloc = instance
            .get_typed_func::<(i32, i32, i32, i32), i32>(&mut store, "cabi_realloc")
            .expect("the Guest Binary exports cabi_realloc");
        let empty = [
            realloc.call(&mut store, (0, 0, 4, 0)).ok(),
            realloc.call(&mut store, (0, 0, 4, 0)).ok(),
        ];
        let past = realloc.call(&mut store, (0, 0, 1, i32::MAX)).is_err();
        assert_eq!(
            (empty, past),
            ([Some(4), Some(4)], true),
            "cabi_realloc asked twice for an empty list on 4 must answer 4 and take no room, and \
             asked for more room than the Guest Binary has must trap rather than answer 0"
        );
    }
}
