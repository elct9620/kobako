//! Host-side dispatch for the `__kobako_dispatch` import.
//!
//! When the guest invokes the wasm import declared in
//! `wasm/kobako-wasm/src/abi.rs`, wasmtime calls back into the host
//! through the closure built in [`super::instance::Instance::build`].
//! That closure delegates here. The dispatcher (docs/behavior.md B-12 / B-13):
//!
//!   1. Reads the Request bytes from guest linear memory.
//!   2. Hands them to the Ruby-side `Kobako::Catalog::Binding` and recovers
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

use core::cell::Cell;
use core::ptr::NonNull;

use magnus::value::{Opaque, ReprValue};
use magnus::{Error as MagnusError, RString, Ruby, Value};
use wasmtime::{Caller, Extern};

use super::host_state::HostState;

// ============================================================
// Thread-local active Caller pointer for yield re-entry (B-24).
// ============================================================
//
// `__kobako_yield_to_block` (the magnus method `Instance#yield_to_block`)
// runs synchronously inside a Ruby Service callback that itself was
// invoked from inside this dispatcher — at that moment we are several
// stack frames deep in `try_handle`, with the original
// `&mut Caller<'_, HostState>` parked unused on the Rust stack while
// Ruby code is running. The yield path needs the same Caller to call
// the guest export, but the Rust borrow type is non-`'static` so it
// cannot be stored in a normal thread-local.
//
// We erase the lifetime to `NonNull<()>` and document the recovery
// invariant at the use site (see [`current_caller`]). The single-
// threaded wasm execution per Sandbox (B-22) plus the LIFO re-entry
// shape ensures no aliasing across threads. The pointer is set on
// entry to [`handle`] and cleared on every exit through a drop guard.

thread_local! {
    static ACTIVE_CALLER: Cell<Option<NonNull<()>>> = const { Cell::new(None) };
}

/// RAII guard that saves the previous [`ACTIVE_CALLER`] value on
/// installation and restores it on drop. Nested `__kobako_dispatch`
/// frames stack — the inner frame's `set` swaps in its own pointer
/// while remembering the outer's; drop restores the outer so the
/// outer's continuation (e.g. iterating over another guest block) still
/// finds a live caller (B-28).
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

/// Recover the active `&mut Caller<'_, HostState>` set by the
/// enclosing [`handle`] frame. Returns `None` when no dispatch frame is
/// active on this thread.
///
/// # Safety
///
/// The returned reference aliases the original `&mut Caller` borrow
/// held on the Rust stack inside [`handle`]'s enclosing frame. The
/// original borrow is logically inactive while Ruby code is running
/// (it is parked on the stack between `invoke_channel` and the eventual
/// `funcall` return), and the single-threaded wasm execution model
/// guarantees no other Rust frame can observe it. Callers must not
/// retain the returned `&mut` past the synchronous Ruby callback that
/// requested it — i.e. only use it inside one short magnus method body
/// and let the borrow end before the method returns.
pub(crate) fn current_caller<'a>() -> Option<&'a mut Caller<'a, HostState>> {
    let raw: NonNull<()> = ACTIVE_CALLER.with(|c| c.get())?;
    // SAFETY: see item doc.
    Some(unsafe { &mut *raw.as_ptr().cast::<Caller<'a, HostState>>() })
}

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
    // SAFETY: lifetime erased to `NonNull<()>` per the module's
    // thread-local doc. The pointer is cleared by `_caller_guard`
    // before this function returns, and only `Instance#yield_to_block`
    // (running inside a Ruby callback we are about to invoke) reads it
    // through `current_caller`.
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

/// Result-returning core of [`handle`]. Pulled out so each early
/// failure path carries a diagnostic string instead of an opaque 0.
fn try_handle(
    caller: &mut Caller<'_, HostState>,
    req_ptr: i32,
    req_len: i32,
) -> Result<i64, &'static str> {
    let req_bytes = read_caller_memory(caller, req_ptr, req_len).ok_or(
        "Sandbox runtime does not export linear memory, or transport request slice falls outside it",
    )?;

    // `Kobako::Sandbox` always installs the Transport Channel before
    // invoking the runtime, so reaching this branch indicates a misuse
    // rather than a normal control path.
    let channel = caller
        .data()
        .channel()
        .ok_or("transport dispatch fired outside an active Sandbox#run — internal wiring bug")?;

    let resp_bytes = invoke_channel(channel, &req_bytes).map_err(|_| {
        "transport channel raised an exception instead of returning a fault — please report this as a kobako bug"
    })?;

    write_response(caller, &resp_bytes)
}

/// Call the Ruby Channel's `#dispatch(request_bytes)` method and return
/// the encoded Response bytes. Errors here mean the Channel itself
/// failed (it is contracted never to raise — see
/// `Kobako::Transport::Channel#dispatch`), which we treat as a wire-layer fault.
fn invoke_channel(channel: Opaque<Value>, req_bytes: &[u8]) -> Result<Vec<u8>, MagnusError> {
    // The wasmtime callback runs on the same Ruby thread that called the
    // active Sandbox invocation (#eval or #run) — the invariant SPEC
    // Implementation Standards Architecture pins for the host gem — so
    // `Ruby::get()` is always available here. Panicking with `expect`
    // localises the violation rather than letting a nonsense error
    // propagate.
    let ruby = Ruby::get().expect("Ruby handle unavailable in __kobako_dispatch");
    let channel_value: Value = ruby.get_inner(channel);
    let req_str = ruby.str_from_slice(req_bytes);
    let resp: RString = channel_value.funcall("dispatch", (req_str,))?;
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
            .map_err(|_| "Sandbox runtime's allocation hook has the wrong signature")?,
        _ => return Err("Sandbox runtime is missing the allocation hook"),
    };
    let len_i32 = i32::try_from(bytes.len()).map_err(|_| "transport response exceeds 2 GiB")?;
    let ptr = alloc
        .call(&mut *caller, len_i32)
        .map_err(|_| "Sandbox allocation trapped while preparing the transport response")?;
    if ptr == 0 {
        return Err("Sandbox is out of memory while preparing the transport response");
    }

    let mem = match caller.get_export("memory") {
        Some(Extern::Memory(m)) => m,
        _ => return Err("Sandbox runtime does not export linear memory"),
    };
    mem.write(&mut *caller, ptr as usize, bytes).map_err(|_| {
        "could not write the transport response into Sandbox memory (range invalid)"
    })?;

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
