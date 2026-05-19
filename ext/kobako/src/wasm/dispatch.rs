//! Host-side dispatch for the `__kobako_dispatch` import.
//!
//! When the guest invokes the wasm import declared in
//! `wasm/kobako-wasm/src/abi.rs`, wasmtime calls back into the host
//! through the closure built in [`super::instance::Instance::build`].
//! That closure delegates here. The dispatcher (docs/behavior.md B-12 / B-13):
//!
//!   1. Reads the Request bytes from guest linear memory.
//!   2. Hands them to the Ruby-side `Kobako::RPC::Server` and recovers
//!      Response bytes.
//!   3. Allocates a guest buffer via `__kobako_alloc(len)` invoked
//!      through `Caller::get_export`.
//!   4. Writes the Response bytes into the guest buffer.
//!   5. Returns packed `(ptr<<32)|len` for the guest to decode.
//!
//! Returns 0 on any step failure. `Kobako::Sandbox` always installs a
//! Server before invoking the guest, so reaching the dispatcher with
//! no Server bound is itself a wire-layer fault; the guest maps a 0
//! return to a trap. Failures during normal dispatch surface as
//! Response.err envelopes from the Server itself — they never reach
//! this 0-return path.
//!
//! ## Why this module writes to `stderr`
//!
//! This file is the one place in `ext/` that deliberately prints
//! through `eprintln!`. The host normally surfaces faults by
//! raising a `MagnusError` back into Ruby; the dispatcher contract
//! is the exception — it must return a packed `i64` to the guest
//! and cannot raise, so a 0 return is the only signal the wasm side
//! receives. The guest collapses every 0 into the same trap, so the
//! Ruby host has no way to attribute the failure to a specific
//! step (missing `memory` export vs. no Server bound vs. Server
//! raised vs. `__kobako_alloc` returned 0 vs. `memory.write`
//! rejected).
//!
//! [`handle`] writes a single `[kobako-dispatch] <reason>` line to
//! `stderr` on each failure path so operators have a breadcrumb to
//! correlate the trap with the actual cause. The line is emitted in
//! both debug and release builds on purpose: dispatcher failures
//! are wire-layer faults rather than expected error paths
//! (`Kobako::Sandbox` always installs a Server, the Server is
//! contracted never to raise, etc.), so the "release-build noise"
//! cost is bounded — under normal operation the line is never
//! written. Operators that need to silence the channel can redirect
//! the host process's stderr, but the kobako convention is "ext
//! never logs" plus this single, named exception.

use magnus::value::{Opaque, ReprValue};
use magnus::{Error as MagnusError, RString, Ruby, Value};
use wasmtime::{Caller, Extern};

use super::host_state::HostState;

/// Drive a single `__kobako_dispatch` invocation end-to-end. Entry point
/// from the wasmtime closure built in [`super::instance::Instance::build`].
///
/// Returns the packed `(ptr<<32)|len` u64 on success, 0 on any
/// wire-layer fault. Failure paths log a `[kobako-dispatch]` line to
/// `stderr` so operators have a breadcrumb when the guest sees a 0
/// return and traps; before this every failure was silent. The Server
/// itself is contracted never to raise (it folds Service exceptions
/// into Response.err envelopes), so reaching the failure path is
/// always a wiring bug or wire-layer fault rather than an expected
/// path.
pub(crate) fn handle(caller: &mut Caller<'_, HostState>, req_ptr: i32, req_len: i32) -> i64 {
    match try_handle(caller, req_ptr, req_len) {
        Ok(packed) => packed,
        Err(reason) => {
            eprintln!("[kobako-dispatch] {}", reason);
            0
        }
    }
}

/// Result-returning core of [`handle`]. Pulled out so each early
/// failure path carries a diagnostic string instead of an opaque 0.
fn try_handle(
    caller: &mut Caller<'_, HostState>,
    req_ptr: i32,
    req_len: i32,
) -> Result<i64, &'static str> {
    let req_bytes = read_caller_memory(caller, req_ptr, req_len)
        .ok_or("guest 'memory' export missing or request slice out of bounds")?;

    // `Kobako::Sandbox` always installs a Server before invoking the
    // guest, so reaching this branch indicates a misuse rather than a
    // normal control path.
    let server = caller
        .data()
        .server()
        .ok_or("no Ruby Server bound — Sandbox#run must precede __kobako_dispatch")?;

    let resp_bytes = invoke_server(server, &req_bytes).map_err(|_| {
        "Ruby Server#dispatch raised — contract is to fold faults into Response.err"
    })?;

    write_response(caller, &resp_bytes)
}

/// Call the Ruby Server's `#dispatch(request_bytes)` method and return
/// the encoded Response bytes. Errors here mean the Server itself
/// failed (it is contracted never to raise — see
/// `Kobako::RPC::Server#dispatch`), which we treat as a wire-layer fault.
fn invoke_server(server: Opaque<Value>, req_bytes: &[u8]) -> Result<Vec<u8>, MagnusError> {
    // The wasmtime callback runs on the same Ruby thread that called the
    // active Sandbox invocation (#eval or #run) — the invariant SPEC
    // Implementation Standards Architecture pins for the host gem — so
    // `Ruby::get()` is always available here. Panicking with `expect`
    // localises the violation rather than letting a nonsense error
    // propagate.
    let ruby = Ruby::get().expect("Ruby handle unavailable in __kobako_dispatch");
    let server_value: Value = ruby.get_inner(server);
    let req_str = ruby.str_from_slice(req_bytes);
    let resp: RString = server_value.funcall("dispatch", (req_str,))?;
    Ok(super::rstring_to_vec(resp))
}

/// Allocate a guest-side buffer through `__kobako_alloc` and copy the
/// response bytes into it. Returns the packed `(ptr<<32)|len` u64.
/// Each failure path carries a `&'static str` reason so the dispatcher
/// wrapper can surface a useful diagnostic rather than a silent 0.
fn write_response(caller: &mut Caller<'_, HostState>, bytes: &[u8]) -> Result<i64, &'static str> {
    let alloc = match caller.get_export("__kobako_alloc") {
        Some(Extern::Func(f)) => f
            .typed::<i32, i32>(&*caller)
            .map_err(|_| "guest '__kobako_alloc' export has wrong signature")?,
        _ => return Err("guest '__kobako_alloc' export missing"),
    };
    let len_i32 = i32::try_from(bytes.len()).map_err(|_| "response exceeds i32::MAX bytes")?;
    let ptr = alloc
        .call(&mut *caller, len_i32)
        .map_err(|_| "__kobako_alloc trapped")?;
    if ptr == 0 {
        return Err("__kobako_alloc returned 0 (out of memory)");
    }

    let mem = match caller.get_export("memory") {
        Some(Extern::Memory(m)) => m,
        _ => return Err("guest 'memory' export missing"),
    };
    mem.write(&mut *caller, ptr as usize, bytes)
        .map_err(|_| "memory.write rejected response buffer range")?;

    let ptr_u32 = ptr as u32;
    let len_u32 = bytes.len() as u32;
    Ok(((ptr_u32 as i64) << 32) | (len_u32 as i64))
}

/// Copy `[ptr, ptr+len)` out of the guest's linear memory as seen from
/// `caller`. Returns `None` when `memory` is not exported or the slice
/// falls outside the live memory range.
fn read_caller_memory(caller: &mut Caller<'_, HostState>, ptr: i32, len: i32) -> Option<Vec<u8>> {
    let mem = match caller.get_export("memory") {
        Some(Extern::Memory(m)) => m,
        _ => return None,
    };
    let data = mem.data(&caller);
    let start = ptr as usize;
    let end = start.checked_add(len as usize)?;
    data.get(start..end).map(|s| s.to_vec())
}
