//! Per-path cache of pre-instantiated wasmtime artifacts.
//!
//! The `Linker` wiring (the WASI preview1 import set plus the
//! `__kobako_dispatch` host import) and its type-check against the
//! compiled Module are identical for every `Kobako::Runtime` on the
//! same Guest Binary — both host closures read all their state from
//! the `Invocation` inside the calling Store, never from the Runtime.
//! Caching the resolved `InstancePre` per path leaves only the
//! `instantiate` call itself on the `Driver::new` hot path.
//!
//! Concurrency: see `crate::cache` — under Ruby's GVL the Mutex serves
//! `Sync` bounds rather than real contention.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use wasmtime::{Caller, InstancePre, Linker};
use wasmtime_wasi::p1;

use crate::cache::{cached_module, shared_engine};
use crate::invocation::Invocation;
use crate::{dispatch, trap};
use kobako_runtime::error::SetupError;

static INSTANCE_PRE_CACHE: OnceLock<Mutex<HashMap<PathBuf, InstancePre<Invocation>>>> =
    OnceLock::new();

/// Look up `path` in the per-path `InstancePre` cache, wiring the
/// Linker and resolving the Module's imports on a miss. Compilation
/// faults surface through `cached_module`; import-resolution faults
/// return a runtime-dead `SetupError` (boundary → `Kobako::SetupError`).
pub(crate) fn cached_instance_pre(path: &Path) -> Result<InstancePre<Invocation>, SetupError> {
    let cache = INSTANCE_PRE_CACHE.get_or_init(|| Mutex::new(HashMap::new()));

    if let Some(pre) = cache
        .lock()
        .expect("instance_pre cache mutex poisoned")
        .get(path)
        .cloned()
    {
        return Ok(pre);
    }

    let module = cached_module(path)?;
    let linker = build_linker()?;
    let pre = linker
        .instantiate_pre(&module)
        .map_err(trap::instantiate_err)?;
    cache
        .lock()
        .expect("instance_pre cache mutex poisoned")
        .insert(path.to_path_buf(), pre.clone());
    Ok(pre)
}

/// Build the host-import `Linker` every Guest Binary instantiates
/// against.
fn build_linker() -> Result<Linker<Invocation>, SetupError> {
    let mut linker: Linker<Invocation> = Linker::new(shared_engine()?);

    // Wire the wasmtime-wasi preview1 WASI imports. Routes guest fd 1/2
    // to the MemoryOutputPipes set up before each run via
    // `Driver::invoke`. The closure pulls a `&mut WasiP1Ctx` out of
    // Invocation; the panic semantics live inside `Invocation::wasi_mut`
    // so the wiring stays honest about its precondition.
    p1::add_to_linker_sync(&mut linker, |state: &mut Invocation| state.wasi_mut())
        .map_err(|e| SetupError::Dead(format!("failed to set up the WASI runtime: {e}")))?;

    // `__kobako_dispatch` host import. Signature per docs/wire-codec.md
    // § ABI Signatures:
    //   (req_ptr: i32, req_len: i32) -> i64
    // Reads the Request bytes from guest memory and hands them —
    // undecoded — to the bound `DispatchHandler` (the frontend's
    // dispatch bridge, e.g. a Ruby Proc), then allocates a guest
    // buffer through `__kobako_alloc`, writes the handler's Response
    // bytes there, and returns the packed `(ptr<<32)|len`. The
    // dispatcher returns 0 on any wire-layer fault (including no
    // handler bound); see `dispatch::handle`.
    linker
        .func_wrap(
            "env",
            "__kobako_dispatch",
            |mut caller: Caller<'_, Invocation>, req_ptr: i32, req_len: i32| -> i64 {
                dispatch::handle(&mut caller, req_ptr, req_len)
            },
        )
        .map_err(|e| SetupError::Dead(format!("failed to set up the host callback bridge: {e}")))?;

    Ok(linker)
}
