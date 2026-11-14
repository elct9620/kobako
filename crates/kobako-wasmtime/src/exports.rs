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
use kobako_transport::abi::DOWNCALL_EXPORT;

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
    pub(crate) downcall: Option<TypedFunc<(i32, i32), u32>>,
    pub(crate) realloc: Option<TypedFunc<(u32, u32, u32, u32), u32>>,
    pub(crate) memory: Option<Memory>,
}

impl Exports {
    /// A missing export is not an error here, because the test fixture is a
    /// bare module; presence is enforced at invocation time.
    pub(crate) fn resolve(instance: &WtInstance, mut ctx: impl AsContextMut) -> Self {
        Self {
            downcall: instance
                .get_typed_func::<(i32, i32), u32>(&mut ctx, DOWNCALL_EXPORT)
                .ok(),
            realloc: instance
                .get_typed_func::<(u32, u32, u32, u32), u32>(&mut ctx, "cabi_realloc")
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
    use kobako_transport::abi::DOWNCALL_EXPORT;
    use wasmtime::{ExternType, Store};

    use crate::cache::{load_engine, load_module};
    use crate::config::Config;
    use crate::guest_binary;
    use crate::imports;
    use crate::invocation::Invocation;
    use crate::trap::NO_TIMEOUT_EPOCH_DELTA;
    use crate::wasi::{sandbox_connections, Wasi};

    const WASM: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/kobako.wasm");

    const ABI_EXPORTS: [&str; 2] = ["cabi_realloc", DOWNCALL_EXPORT];

    // @behavior RT-068
    #[test]
    fn the_guest_binary_exports_the_abi_functions_and_nothing_else() {
        let path = Path::new(WASM);
        if !guest_binary::built(path.exists(), "data/kobako.wasm") {
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
        if !guest_binary::built(path.exists(), "data/kobako.wasm") {
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

    /// Where a function's calls can lead: the imports it reaches, and whether
    /// it reaches a call through the function table, which could be any.
    #[derive(Debug, Default, PartialEq)]
    struct Reach {
        imports: Vec<String>,
        indirect: bool,
    }

    /// What the export `name` of the module in `bytes` can reach through its
    /// direct calls.
    fn reach_of(bytes: &[u8], name: &str) -> Reach {
        use wasmparser::{ExternalKind, Operator, Parser, Payload, TypeRef};

        let (mut imports, mut start) = (Vec::new(), None);
        let mut bodies: Vec<(Vec<u32>, bool)> = Vec::new();
        for payload in Parser::new(0).parse_all(bytes) {
            match payload.expect("the Guest Binary parses") {
                Payload::ImportSection(section) => {
                    for import in section.into_imports() {
                        let import = import.expect("an import parses");
                        if matches!(import.ty, TypeRef::Func(_)) {
                            imports.push(format!("{}.{}", import.module, import.name));
                        }
                    }
                }
                Payload::ExportSection(section) => {
                    for export in section {
                        let export = export.expect("an export parses");
                        if export.name == name && export.kind == ExternalKind::Func {
                            start = Some(export.index);
                        }
                    }
                }
                Payload::CodeSectionEntry(body) => {
                    let (mut calls, mut indirect) = (Vec::new(), false);
                    let mut reader = body.get_operators_reader().expect("a body parses");
                    while !reader.eof() {
                        match reader.read().expect("an operator parses") {
                            Operator::Call { function_index }
                            | Operator::ReturnCall { function_index } => calls.push(function_index),
                            Operator::CallIndirect { .. }
                            | Operator::ReturnCallIndirect { .. }
                            | Operator::CallRef { .. }
                            | Operator::ReturnCallRef { .. } => indirect = true,
                            _ => {}
                        }
                    }
                    bodies.push((calls, indirect));
                }
                _ => {}
            }
        }

        let mut reach = Reach::default();
        let mut seen = std::collections::HashSet::new();
        let mut pending = vec![start.unwrap_or_else(|| panic!("the module exports {name}"))];
        while let Some(index) = pending.pop() {
            if !seen.insert(index) {
                continue;
            }
            match imports.get(index as usize) {
                Some(import) => reach.imports.push(import.clone()),
                None => {
                    let (calls, indirect) = &bodies[index as usize - imports.len()];
                    reach.indirect |= indirect;
                    pending.extend(calls);
                }
            }
        }
        reach.imports.sort_unstable();
        reach
    }

    // @behavior RT-397
    #[test]
    fn the_guest_binarys_allocator_calls_into_no_host_import() {
        let path = Path::new(WASM);
        if !guest_binary::built(path.exists(), "data/kobako.wasm") {
            return;
        }
        let bytes = std::fs::read(path).expect("the Guest Binary must read");

        assert_eq!(
            reach_of(&bytes, "cabi_realloc"),
            Reach::default(),
            "cabi_realloc in data/kobako.wasm, followed through its calls, must reach no host \
             import and no call through the function table"
        );
    }
}
