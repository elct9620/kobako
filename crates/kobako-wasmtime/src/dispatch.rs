//! Host side of the `__kobako_dispatch` import: hand the guest's Call to
//! the bound `DispatchHandler` and write its Reply back into guest memory.
//!
//! A failure lands on whoever is answerable for it. A trap the guest
//! raises while the host calls back into it ends the invocation as that
//! trap; any other failure is the guest's or the wire's, so the import
//! answers 0, which the guest receives as a wire failure.
//!
//! A 0 carries no reason, so each one also writes a single
//! `[kobako-dispatch] <reason>` line to stderr. It is the one place the
//! driver logs, and normal operation never reaches it.

use wasmtime::Caller;

use kobako_transport::envelope::Call;

use crate::invocation::Invocation;

/// Answer one `__kobako_dispatch` call: the packed `(ptr<<32)|len` of the
/// Reply, 0 for a failure the guest or the wire is answerable for, or the
/// trap a callback into the guest raised.
pub(crate) fn handle(
    caller: &mut Caller<'_, Invocation>,
    req_ptr: i32,
    req_len: i32,
) -> wasmtime::Result<i64> {
    let answer = try_handle(caller, req_ptr, req_len);
    if let Some(trap) = caller.data_mut().take_reentry_trap() {
        return Err(trap);
    }
    Ok(answer.unwrap_or_else(|reason| {
        eprintln!("[kobako-dispatch] {reason}");
        0
    }))
}

/// The exchange itself, each failure carrying the reason `handle` logs.
fn try_handle(
    caller: &mut Caller<'_, Invocation>,
    req_ptr: i32,
    req_len: i32,
) -> Result<i64, &'static str> {
    let req_bytes = crate::guest_mem::read(caller, req_ptr, req_len)?;
    // The driver decodes the core envelope so the frontend never sees a
    // frame; the payload inside it stays bytes the whole way through.
    let call = Call::decode(&req_bytes).map_err(|_| {
        "the guest sent a malformed Call envelope — please report this as a kobako bug"
    })?;

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
    // the LIFO re-entry lives on the Rust stack — no shared slot.
    let reply = {
        let mut yielder = crate::guest_mem::CallerYielder::new(caller);
        handler.dispatch(call, &mut yielder)
    }
    .ok_or(
        "a Sandbox callback raised an exception instead of returning a fault — please report this as a kobako bug",
    )?;

    write_response(caller, &reply.encode())
}

/// Allocate a guest-side buffer and copy the response bytes into it via
/// `crate::guest_mem::alloc_and_write`, returning the packed
/// `(ptr<<32)|len` u64 the guest's `__kobako_dispatch` import expects.
fn write_response(caller: &mut Caller<'_, Invocation>, bytes: &[u8]) -> Result<i64, &'static str> {
    let ptr = crate::guest_mem::alloc_and_write(caller, bytes)?;
    Ok(((ptr as i64) << 32) | (bytes.len() as i64))
}
