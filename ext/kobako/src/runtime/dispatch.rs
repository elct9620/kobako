//! Host-side dispatch for the `__kobako_dispatch` import.
//!
//! When the guest invokes the wasm import declared in
//! `wasm/kobako-core/src/abi.rs`, wasmtime calls back into the host
//! through the closure registered by `instance_pre::build_linker`.
//! That closure delegates here. The dispatcher:
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
use magnus::{method, prelude::*, Error as MagnusError, RClass, RString, Ruby, Value};
use wasmtime::Caller;

use super::invocation::Invocation;
use crate::contract::dispatch::DispatchHandler;
use crate::contract::yielder::Yielder;

/// Register the `Kobako::Runtime::GuestYielder` Ruby class. Called from
/// `crate::runtime::init` after `Kobako::Runtime` is defined so the
/// `#[magnus::wrap]` class name resolves before any object is wrapped.
pub(super) fn register(runtime_class: RClass) -> Result<(), MagnusError> {
    let ruby = Ruby::get().expect("Ruby thread");
    let class = runtime_class.define_class("GuestYielder", ruby.class_object())?;
    class.define_method("call", method!(GuestYielder::call, 1))?;
    Ok(())
}

/// Frame-scoped Ruby handle that lets the dispatch `Proc` re-enter the
/// guest to run a yielded block. It wraps the active `&mut dyn Yielder`
/// for exactly one `__kobako_dispatch` frame: the bridge builds one, hands
/// it to the `Proc` as the second argument, and `invalidate`s it the
/// instant the `Proc` returns. A guest block stashed and called after that
/// frame (E-23) normally raises `LocalJumpError` at the Ruby
/// `Transport::Yielder` net — invalidated in the dispatcher's `ensure`,
/// which fires before this handle is reached. This inner invalidation is
/// the backstop behind that outer net: it keeps `call`'s `unsafe`
/// `NonNull` deref from touching freed stack should the outer net ever be
/// bypassed, so neither net is redundant.
///
/// This is the single, explicit, frame-scoped FFI pointer the host↔guest
/// re-entry still costs: `magnus`' `funcall` sits between two Rust frames,
/// so the typed `&mut dyn Yielder` cannot cross it and is erased to a raw
/// pointer here. Unlike the dispatch `Proc`, this handle holds **no Ruby
/// `Value`**, so GC has nothing to trace through it — it needs no `mark`.
#[magnus::wrap(class = "Kobako::Runtime::GuestYielder", free_immediately, size)]
struct GuestYielder {
    yielder: Cell<Option<NonNull<dyn Yielder>>>,
}

// SAFETY: magnus requires `Send + Sync` on wrapped types. The raw pointer
// is created, used, and invalidated within a single `__kobako_dispatch`
// frame on the one Ruby thread that owns the active Invocation (SPEC.md
// Single-Invocation Slot); it is never read from another thread.
unsafe impl Send for GuestYielder {}
unsafe impl Sync for GuestYielder {}

impl GuestYielder {
    /// Erase the frame-scoped `&mut dyn Yielder` into a Ruby-owned handle.
    /// Safety contract for the caller: `invalidate` MUST run before the
    /// borrow this pointer came from ends (i.e. before the dispatch frame
    /// returns).
    fn new(yielder: &mut dyn Yielder) -> Self {
        let ptr = NonNull::from(yielder);
        // Erase the borrow's lifetime to `'static`; the pointer is only
        // ever dereferenced while it is still `Some` (i.e. `invalidate`
        // has not run), so the referent is guaranteed live.
        let ptr: NonNull<dyn Yielder> = unsafe {
            std::mem::transmute::<NonNull<dyn Yielder + '_>, NonNull<dyn Yielder + 'static>>(ptr)
        };
        Self {
            yielder: Cell::new(Some(ptr)),
        }
    }

    /// Mark this handle dead. Called the instant the dispatch frame's
    /// `funcall` returns, so a guest block stashed beyond its frame raises
    /// instead of dereferencing freed stack.
    fn invalidate(&self) {
        self.yielder.set(None);
    }

    /// Ruby-visible `call(args_bytes) -> resp_bytes`: drive one yield
    /// round-trip. Stands in for the `String -> String` callable the host
    /// `Transport::Yielder` invokes. Raises `Kobako::TrapError` when the
    /// handle has been invalidated (escaped guest block) or the re-entry
    /// itself traps.
    fn call(&self, args: RString) -> Result<RString, MagnusError> {
        let ruby = Ruby::get().expect("Ruby handle unavailable in __kobako_yield");
        let Some(mut ptr) = self.yielder.get() else {
            return Err(super::errors::trap_err(
                &ruby,
                "guest block invoked after the host dispatch frame returned",
            ));
        };
        let bytes = super::rstring_to_vec(args);
        // SAFETY: `yielder` is `Some`, so `invalidate` has not run — the
        // dispatch frame that lent the `&mut dyn Yielder` is still on the
        // Rust stack, and the Single-Invocation Slot guarantees no other
        // frame aliases it. The borrow ends with this method.
        let yielder: &mut dyn Yielder = unsafe { ptr.as_mut() };
        let resp = yielder
            .yield_block(&bytes)
            .map_err(|t| super::errors::trap_to_magnus(&ruby, t))?;
        Ok(ruby.str_from_slice(&resp))
    }
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
pub(super) fn handle(caller: &mut Caller<'_, Invocation>, req_ptr: i32, req_len: i32) -> i64 {
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
    let req_bytes = super::guest_mem::read(caller, req_ptr, req_len)?;

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
        let mut yielder = super::guest_mem::CallerYielder::new(caller);
        handler.dispatch(&req_bytes, &mut yielder)
    }
    .ok_or(
        "a Sandbox callback raised an exception instead of returning a fault — please report this as a kobako bug",
    )?;

    write_response(caller, &resp_bytes)
}

/// The Ruby-Proc bridge: a `DispatchHandler` backed by the host-side
/// dispatch `Proc` registered through `Runtime#on_dispatch=`. This is the
/// one place the dispatch seam touches `magnus`; the wasm runtime sees
/// only the trait. The Proc is GC-rooted by `Runtime`'s `mark`; this
/// struct holds an `Opaque` copy of the same handle.
pub(super) struct RubyDispatchHandler {
    on_dispatch: Opaque<Value>,
}

impl RubyDispatchHandler {
    pub(super) fn new(on_dispatch: Opaque<Value>) -> Self {
        Self { on_dispatch }
    }
}

impl DispatchHandler for RubyDispatchHandler {
    /// Call the Ruby Proc with the request bytes and return the encoded
    /// Response bytes. The Proc is contracted to fold every dispatch
    /// failure into a `Response.err` envelope (see
    /// `Kobako::Transport::Dispatcher.dispatch`), so a raise is a contract
    /// violation surfaced as `None` — the dispatcher then walks the
    /// 0-return wire-fault path.
    fn dispatch(&self, request: &[u8], yielder: &mut dyn Yielder) -> Option<Vec<u8>> {
        // The wasmtime callback runs on the same Ruby thread that called
        // the active Sandbox invocation (#eval or #run) — the invariant
        // SPEC Implementation Standards Architecture pins for the host gem
        // — so `Ruby::get()` is always available here. Panicking with
        // `expect` localises the violation rather than letting a nonsense
        // error propagate.
        let ruby = Ruby::get().expect("Ruby handle unavailable in __kobako_dispatch");
        let proc_value: Value = ruby.get_inner(self.on_dispatch);
        let req_str = ruby.str_from_slice(request);
        // Hand the Proc a frame-scoped yielder object as its second arg and
        // invalidate it the instant the Proc returns, so a guest block that
        // escapes the dispatch frame can never deref the freed stack
        // pointer. `guest_yielder` holds no Ruby Value, so it needs no GC
        // mark — the GC has nothing to trace through it.
        let guest_yielder = ruby.obj_wrap(GuestYielder::new(yielder));
        let resp: Result<RString, magnus::Error> =
            proc_value.funcall("call", (req_str, guest_yielder));
        guest_yielder.invalidate();
        resp.ok().map(super::rstring_to_vec)
    }
}

/// Allocate a guest-side buffer and copy the response bytes into it via
/// `super::guest_mem::alloc_and_write`, returning the packed
/// `(ptr<<32)|len` u64 the guest's `__kobako_dispatch` import expects.
fn write_response(caller: &mut Caller<'_, Invocation>, bytes: &[u8]) -> Result<i64, &'static str> {
    let ptr = super::guest_mem::alloc_and_write(caller, bytes)?;
    Ok(((ptr as i64) << 32) | (bytes.len() as i64))
}
