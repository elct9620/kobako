//! Host-side dispatch for the `__kobako_dispatch` import.
//!
//! When the guest invokes the wasm import declared in
//! `wasm/kobako-core/src/abi.rs`, wasmtime calls back into the host
//! through the closure registered by `instance_pre::build_linker`.
//! That closure delegates here. The dispatcher (docs/behavior.md B-12 / B-13):
//!
//!   1. Reads the Request bytes from guest linear memory.
//!   2. Invokes the Ruby-side dispatch Proc bound via
//!      `Runtime#on_dispatch=` and recovers Response bytes.
//!   3. Allocates a guest buffer via `__kobako_alloc(len)` invoked
//!      through `Caller::get_export`.
//!   4. Writes the Response bytes into the guest buffer.
//!   5. Returns packed `(ptr<<32)|len` for the guest to decode.
//!
//! Returns 0 on any step failure. `Kobako::Sandbox#initialize` always
//! installs the dispatch Proc before any invocation, so reaching the
//! dispatcher with no Proc bound is itself a wire-layer fault; the
//! guest maps a 0 return to a trap. Failures during normal dispatch
//! surface as Response.err envelopes from
//! `Kobako::Transport::Dispatcher.dispatch` itself — they never reach
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
//! Ruby host has no way to attribute the failure to a specific step
//! (missing `memory` export vs. no dispatch Proc bound vs. the Proc
//! raised vs. `__kobako_alloc` returned 0 vs. `memory.write`
//! rejected).
//!
//! `handle` writes a single `[kobako-dispatch] <reason>` line to
//! `stderr` on each failure path so operators have a breadcrumb to
//! correlate the trap with the actual cause. The line is emitted in
//! both debug and release builds on purpose: dispatcher failures are
//! wire-layer faults rather than expected error paths (`Kobako::Sandbox`
//! always installs the Proc, the Proc is contracted never to raise,
//! etc.), so the "release-build noise" cost is bounded — under normal
//! operation the line is never written. Operators that need to silence
//! the stream can redirect the host process's stderr, but the kobako
//! convention is "ext never logs" plus this single, named exception.

use core::cell::Cell;
use core::ptr::NonNull;

use magnus::value::{Opaque, ReprValue};
use magnus::{Error as MagnusError, RString, Ruby, Value};
use wasmtime::Caller;

use super::invocation::Invocation;

// ============================================================
// Active-caller pointer for the per-thread Invocation slot (B-24, B-28,
// SPEC.md Single-Invocation Slot).
// ============================================================
//
// `Runtime#yield_to_active_invocation` (whose body is the
// `__kobako_yield_to_block` guest export) runs synchronously inside a
// Ruby Service callback that itself was invoked from inside this
// dispatcher — at that moment we are several stack frames deep in
// `try_handle`, with the original `&mut Caller<'_, Invocation>` parked
// unused on the Rust stack while Ruby code is running. The yield path
// needs the same Caller to call the guest export, but the Rust borrow
// type is non-`'static` so it cannot be stored on the `Invocation`
// struct directly (the `&mut Caller` outlives no struct field — its
// lifetime ends when `handle` returns to wasmtime).
//
// The pointer is therefore erased to `NonNull<()>` and parked in a
// per-thread slot — the materialised form of the SPEC.md
// "Single-Invocation Slot" invariant. The single-threaded wasm
// execution per Sandbox (B-22) plus the LIFO re-entry shape of nested
// dispatch frames (B-28) ensures no aliasing across threads or across
// frames; the recovery invariant lives at `current_caller`. The
// pointer is set on entry to `handle` and restored to the outer
// frame's value on every exit through a drop guard.

thread_local! {
    static ACTIVE_CALLER: Cell<Option<NonNull<()>>> = const { Cell::new(None) };
}

/// RAII guard that saves the previous `ACTIVE_CALLER` value on
/// installation and restores it on drop. Nested `__kobako_dispatch`
/// frames stack within one Invocation (B-28) — the inner frame's `set`
/// swaps in its own pointer while remembering the outer's; drop
/// restores the outer so its continuation (e.g. iterating over another
/// guest block) still finds a live caller.
pub(crate) struct CallerGuard {
    previous: Option<NonNull<()>>,
}

impl CallerGuard {
    fn set(ptr: NonNull<()>) -> Self {
        let previous = ACTIVE_CALLER.with(|c| c.replace(Some(ptr)));
        Self { previous }
    }
}

impl Drop for CallerGuard {
    fn drop(&mut self) {
        ACTIVE_CALLER.with(|c| c.set(self.previous));
    }
}

/// Recover the active `&mut Caller<'_, Invocation>` set by the
/// enclosing `handle` frame. Returns `None` when no dispatch frame is
/// active on this thread.
///
/// # Safety
///
/// The returned reference aliases the original `&mut Caller` borrow
/// held on the Rust stack inside `handle`'s enclosing frame. The
/// original borrow is logically inactive while Ruby code is running
/// (it is parked on the stack between `invoke_on_dispatch` and the
/// eventual `funcall` return), and the SPEC.md Single-Invocation Slot
/// invariant (one Invocation per OS thread for the duration of any
/// invocation) guarantees no other Rust frame can observe it. Callers
/// must not retain the returned `&mut` past the synchronous Ruby
/// callback that requested it — i.e. only use it inside one short
/// magnus method body and let the borrow end before the method returns.
pub(crate) fn current_caller<'a>() -> Option<&'a mut Caller<'a, Invocation>> {
    let raw: NonNull<()> = ACTIVE_CALLER.with(|c| c.get())?;
    // SAFETY: see item doc.
    Some(unsafe { &mut *raw.as_ptr().cast::<Caller<'a, Invocation>>() })
}

/// Drive a single `__kobako_dispatch` invocation end-to-end. Entry point
/// from the wasmtime closure registered by `instance_pre::build_linker`.
///
/// Returns the packed `(ptr<<32)|len` u64 on success, 0 on any
/// wire-layer fault. Failure paths log a `[kobako-dispatch]` line to
/// `stderr` so operators have a breadcrumb when the guest sees a 0
/// return and traps. The bound dispatch Proc is contracted never to
/// raise (it folds Service exceptions into Response.err envelopes),
/// so reaching the failure path is always a wiring bug or wire-layer
/// fault rather than an expected path.
pub(crate) fn handle(caller: &mut Caller<'_, Invocation>, req_ptr: i32, req_len: i32) -> i64 {
    // SAFETY: lifetime erased to `NonNull<()>` per the module's
    // Invocation-slot doc. The pointer is restored by `_caller_guard`
    // before this function returns, and only
    // `Runtime#yield_to_active_invocation` (running inside a Ruby
    // callback we are about to invoke) reads it through `current_caller`.
    let ptr: NonNull<()> = NonNull::from(&mut *caller).cast();
    let _caller_guard = CallerGuard::set(ptr);

    match try_handle(caller, req_ptr, req_len) {
        Ok(packed) => packed,
        Err(reason) => {
            eprintln!("[kobako-dispatch] {}", reason);
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
    let req_bytes = super::guest_mem::read(caller, req_ptr, req_len)?;

    // `Kobako::Sandbox` always installs the dispatch Proc before
    // invoking the runtime, so reaching this branch indicates a misuse
    // rather than a normal control path.
    let on_dispatch = caller
        .data()
        .on_dispatch()
        .ok_or("a Sandbox callback fired outside an active Sandbox#run — please report this as a kobako bug")?;

    let resp_bytes = invoke_on_dispatch(on_dispatch, &req_bytes).map_err(|_| {
        "a Sandbox callback raised an exception instead of returning a fault — please report this as a kobako bug"
    })?;

    write_response(caller, &resp_bytes)
}

/// Invoke the Ruby-side dispatch `Proc` with the request bytes and return
/// the encoded Response bytes. The Proc is contracted to fold every
/// dispatch failure into a `Response.err` envelope (see
/// `Kobako::Transport::Dispatcher.dispatch`), so reaching the error
/// branch is itself a wire-layer fault rather than a normal control path.
fn invoke_on_dispatch(
    on_dispatch: Opaque<Value>,
    req_bytes: &[u8],
) -> Result<Vec<u8>, MagnusError> {
    // The wasmtime callback runs on the same Ruby thread that called the
    // active Sandbox invocation (#eval or #run) — the invariant SPEC
    // Implementation Standards Architecture pins for the host gem — so
    // `Ruby::get()` is always available here. Panicking with `expect`
    // localises the violation rather than letting a nonsense error
    // propagate.
    let ruby = Ruby::get().expect("Ruby handle unavailable in __kobako_dispatch");
    let proc_value: Value = ruby.get_inner(on_dispatch);
    let req_str = ruby.str_from_slice(req_bytes);
    let resp: RString = proc_value.funcall("call", (req_str,))?;
    Ok(super::rstring_to_vec(resp))
}

/// Allocate a guest-side buffer and copy the response bytes into it via
/// `super::guest_mem::alloc_and_write`, returning the packed
/// `(ptr<<32)|len` u64 the guest's `__kobako_dispatch` import expects.
fn write_response(caller: &mut Caller<'_, Invocation>, bytes: &[u8]) -> Result<i64, &'static str> {
    let ptr = super::guest_mem::alloc_and_write(caller, bytes)?;
    Ok(((ptr as i64) << 32) | (bytes.len() as i64))
}
