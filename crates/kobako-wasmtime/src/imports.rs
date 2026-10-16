//! The host imports a Guest Binary resolves: the WASI preview1 set and
//! the wire ABI's `__kobako_upcall`.
//!
//! Both closures read all their state from the `Invocation` inside the
//! calling Store, never from a Runtime, so the wiring is the same for
//! every Sandbox and `crate::instance_pre` can cache what it resolves.

use wasmtime::{Caller, Linker};
use wasmtime_wasi::p1;

use crate::cache::shared_engine;
use crate::dispatch;
use crate::invocation::Invocation;
use kobako_runtime::error::SetupError;

pub(crate) fn linker() -> Result<Linker<Invocation>, SetupError> {
    let mut linker: Linker<Invocation> = Linker::new(shared_engine()?);

    // Wire the wasmtime-wasi preview1 WASI imports against each
    // invocation's own `Wasi` context, which routes guest fd 1/2 to its
    // capture pipes.
    p1::add_to_linker_sync(&mut linker, |state: &mut Invocation| {
        state.wasi_mut().ctx_mut()
    })
    .map_err(|e| SetupError::Dead(format!("failed to set up the WASI runtime: {e}")))?;

    // `__kobako_upcall`, `(ptr: i32, len: i32) -> i64` per
    // docs/wire/abi.md § ABI Signatures; `dispatch::handle` owns what it
    // answers.
    linker
        .func_wrap(
            "env",
            "__kobako_upcall",
            |mut caller: Caller<'_, Invocation>,
             req_ptr: i32,
             req_len: i32|
             -> wasmtime::Result<i64> {
                dispatch::handle(&mut caller, req_ptr, req_len)
            },
        )
        .map_err(|e| SetupError::Dead(format!("failed to set up the host callback bridge: {e}")))?;

    Ok(linker)
}
