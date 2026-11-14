//! The host imports a Guest Binary resolves: the WASI preview1 set, the
//! WASI 0.2 functions a core module links, and the kobako ABI's `upcall`
//! and `preamble`.
//!
//! Every import reads its state from the `Invocation` inside the calling
//! Store, never from a Runtime, so the wiring is the same for every Sandbox
//! and `crate::instance_pre` can cache what it resolves.

use wasmtime::{AsContextMut, Caller, Linker};
use wasmtime_wasi::p1;

use crate::cache::load_engine;
use crate::guest::Guest;
use crate::invocation::Invocation;
use crate::preview1;
use crate::preview2;
use kobako_runtime::endpoint::{self, ImportEnd, ABANDONED};
use kobako_runtime::error::SetupError;
use kobako_transport::abi::{HOST_INTERFACE, PREAMBLE, UPCALL};

pub(crate) fn linker() -> Result<Linker<Invocation>, SetupError> {
    let mut linker: Linker<Invocation> = Linker::new(load_engine()?);

    // Wire the wasmtime-wasi preview1 WASI imports against each
    // invocation's own `Wasi` context, then answer every call that reads
    // a descriptor from kobako's own table, which routes guest fd 1/2 to
    // its capture pipes.
    p1::add_to_linker_sync(&mut linker, |state: &mut Invocation| {
        state.wasi_mut().ctx_mut()
    })
    .and_then(|_| preview1::shadow(&mut linker))
    .and_then(|_| preview2::add_to_linker(&mut linker))
    .map_err(|e| SetupError::Dead(format!("failed to set up the WASI runtime: {e}")))?;

    // `upcall`, `(ptr: i32, len: i32, ret: i32)` in the ABI, and `preamble`,
    // `(ret: i32)`; the endpoint owns what each answers.
    let bridge = |e: wasmtime::Error| {
        SetupError::Dead(format!("failed to set up the host callback bridge: {e}"))
    };
    linker
        .func_wrap(
            HOST_INTERFACE,
            UPCALL,
            |mut caller: Caller<'_, Invocation>,
             req_ptr: i32,
             req_len: i32,
             ret: i32|
             -> wasmtime::Result<()> {
                let mut guest = Guest(caller.as_context_mut());
                endpoint::handle(&mut guest, req_ptr, req_len, ret).map_err(to_import_error)
            },
        )
        .map_err(bridge)?;
    linker
        .func_wrap(
            HOST_INTERFACE,
            PREAMBLE,
            |mut caller: Caller<'_, Invocation>, ret: i32| -> wasmtime::Result<()> {
                let preamble = caller.data().preamble();
                let mut guest = Guest(caller.as_context_mut());
                endpoint::hand_preamble(&mut guest, &preamble, ret).map_err(to_import_error)
            },
        )
        .map_err(bridge)?;

    Ok(linker)
}

/// The trap a host import ends the invocation with instead of answering.
fn to_import_error(end: ImportEnd<wasmtime::Error>) -> wasmtime::Error {
    match end {
        ImportEnd::Fault(trap) => trap,
        ImportEnd::Abandoned => wasmtime::Error::msg(ABANDONED),
        ImportEnd::Unwritable(reason) => wasmtime::Error::msg(reason),
    }
}
