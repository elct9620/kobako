//! Host-side dispatch for the `__kobako_dispatch` import.
//!
//! When the guest invokes the wasm import declared in
//! `wasm/kobako-core/src/abi.rs`, wasmtime calls back into the host
//! through the closure registered by `instance_pre::build_linker`.
//! That closure delegates here. The dispatcher:
//!
//!   1. Reads the Request bytes from guest linear memory.
//!   2. Invokes the bound `DispatchHandler` (the frontend's dispatch
//!      bridge, e.g. a Ruby Proc) and recovers Response bytes.
//!   3. Allocates a guest buffer via `__kobako_alloc(len)` invoked
//!      through `Caller::get_export`.
//!   4. Writes the Response bytes into the guest buffer.
//!   5. Returns packed `(ptr<<32)|len` for the guest to decode.
//!
//! Returns 0 on any step failure. `Kobako::Sandbox#initialize` always
//! installs the dispatch handler before any invocation, so reaching the
//! dispatcher with no handler bound is itself a wire-layer fault; the
//! guest maps a 0 return to a trap. Failures during normal dispatch
//! surface as Response.err envelopes from
//! `Kobako::Transport::Dispatcher.dispatch` itself — they never reach
//! this 0-return path.
//!
//! ## Why this module writes to `stderr`
//!
//! This file is the one place in the driver that deliberately prints
//! through `eprintln!`. The host normally surfaces faults through the
//! contract's error channels; the dispatcher contract is the exception
//! — it must return a packed `i64` to the guest and cannot fail, so a
//! 0 return is the only signal the wasm side receives. The guest collapses every 0 into the same trap, so the
//! Ruby host has no way to attribute the failure to a specific step
//! (missing `memory` export vs. no dispatch handler bound vs. the
//! handler raised vs. `__kobako_alloc` returned 0 vs. `memory.write`
//! rejected).
//!
//! `handle` writes a single `[kobako-dispatch] <reason>` line to
//! `stderr` on each failure path so operators have a breadcrumb to
//! correlate the trap with the actual cause. The line is emitted in
//! both debug and release builds on purpose: dispatcher failures are
//! wire-layer faults rather than expected error paths (`Kobako::Sandbox`
//! always installs the handler, the handler is contracted never to
//! raise, etc.), so the "release-build noise" cost is bounded — under
//! normal operation the line is never written. Operators that need to
//! silence the stream can redirect the host process's stderr, but the
//! kobako convention is "ext never logs" plus this single, named
//! exception.

use wasmtime::Caller;

use crate::invocation::Invocation;

/// Drive a single `__kobako_dispatch` invocation end-to-end. Entry point
/// from the wasmtime closure registered by `instance_pre::build_linker`.
///
/// Returns the packed `(ptr<<32)|len` u64 on success, 0 on any
/// wire-layer fault. Failure paths log a `[kobako-dispatch]` line to
/// `stderr` so operators have a breadcrumb when the guest sees a 0
/// return and traps. The bound dispatch handler is contracted never to
/// raise (it folds Service exceptions into Response.err envelopes),
/// so reaching the failure path is always a wiring bug or wire-layer
/// fault rather than an expected path.
pub(crate) fn handle(caller: &mut Caller<'_, Invocation>, req_ptr: i32, req_len: i32) -> i64 {
    match try_handle(caller, req_ptr, req_len) {
        Ok(packed) => packed,
        Err(reason) => {
            eprintln!("[kobako-dispatch] {reason}");
            0
        }
    }
}

/// Result-returning core of `handle`. Pulled out so each early
/// failure path carries a diagnostic string instead of an opaque 0.
fn try_handle(
    caller: &mut Caller<'_, Invocation>,
    req_ptr: i32,
    req_len: i32,
) -> Result<i64, &'static str> {
    let req_bytes = crate::guest_mem::read(caller, req_ptr, req_len)?;

    // `Kobako::Sandbox` always installs the dispatch handler before
    // invoking the runtime, so reaching this branch indicates a misuse
    // rather than a normal control path.
    let handler = caller
        .data()
        .on_dispatch()
        .ok_or("a Sandbox callback fired outside an active Sandbox#run — please report this as a kobako bug")?;

    // Build a frame-scoped yielder over this Caller and hand it to the
    // handler. The borrow ends with the block, freeing the Caller for
    // `write_response`; nested dispatch frames each build their own, so
    // the LIFO re-entry lives on the Rust stack — no shared slot (B-28).
    let resp_bytes = {
        let mut yielder = crate::guest_mem::CallerYielder::new(caller);
        handler.dispatch(&req_bytes, &mut yielder)
    }
    .ok_or(
        "a Sandbox callback raised an exception instead of returning a fault — please report this as a kobako bug",
    )?;

    write_response(caller, &resp_bytes)
}

/// Allocate a guest-side buffer and copy the response bytes into it via
/// `crate::guest_mem::alloc_and_write`, returning the packed
/// `(ptr<<32)|len` u64 the guest's `__kobako_dispatch` import expects.
fn write_response(caller: &mut Caller<'_, Invocation>, bytes: &[u8]) -> Result<i64, &'static str> {
    let ptr = crate::guest_mem::alloc_and_write(caller, bytes)?;
    Ok(((ptr as i64) << 32) | (bytes.len() as i64))
}
