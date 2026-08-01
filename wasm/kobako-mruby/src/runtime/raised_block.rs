//! The exception a guest block raised, held across the host round-trip.
//!
//! A block that raises does not end the invocation: the host re-raises at
//! the Service's `yield` site, and the Service may rescue it (B-24). Only
//! an unrescued one comes back, and when it does the failure is the
//! caller's own — so the caller re-raises the exception it raised rather
//! than a reconstruction of it, which is what the same two frames would
//! do with no Sandbox between them.
//!
//! One slot, not a stack: a nested dispatch completes before the frame
//! that contains it resumes, so the set and the take of any one exception
//! are adjacent in time. `crate::runtime::bridges` takes it on every
//! answer, whatever the arm, so a Service that rescued the failure and
//! returned leaves nothing behind for a later answer to re-raise.
//!
//! ## Cross-Sandbox isolation
//!
//! Same argument as `crate::runtime::block_stack`: each `Kobako::Sandbox`
//! owns its own `wasmtime::Instance` and therefore its own copy of this
//! module-level static.

use beni::Value;

use core::cell::UnsafeCell;

/// Single-threaded interior-mutability slot for the exception the most
/// recent guest block raised.
pub(crate) struct RaisedBlock(UnsafeCell<Option<Value>>);

impl RaisedBlock {
    const fn new() -> Self {
        Self(UnsafeCell::new(None))
    }

    /// Record `exc` as the exception the running block raised, rooting it
    /// against GC: the mruby frame that raised has already unwound by the
    /// time the host sees the failure, so nothing else keeps it alive.
    /// Any exception already held is released — a Service that rescued
    /// the earlier one and yielded again has handled it.
    #[cfg(mruby_linked)]
    pub(crate) fn set(&self, mrb: &beni::Mrb, exc: Value) {
        // SAFETY: see type doc.
        let held = unsafe { (*self.0.get()).replace(exc) };
        unroot(mrb, held);
        // SAFETY: `exc` is a live exception on this VM; the paired
        // unregister runs in `take` or in the next `set`.
        unsafe { beni::sys::mrb_gc_register(mrb.as_ptr(), exc.as_raw()) };
    }

    /// Take the held exception, releasing the GC root. The caller either
    /// re-raises it — which roots it again through mruby's own exception
    /// slot — or drops it, having answered on an arm that is not the
    /// block's failure.
    pub(crate) fn take(&self, mrb: &beni::Mrb) -> Option<Value> {
        // SAFETY: see type doc.
        let exc = unsafe { (*self.0.get()).take() }?;
        unroot(mrb, Some(exc));
        Some(exc)
    }
}

#[cfg(mruby_linked)]
fn unroot(mrb: &beni::Mrb, held: Option<Value>) {
    if let Some(exc) = held {
        // SAFETY: paired with the register in `set`.
        unsafe { beni::sys::mrb_gc_unregister(mrb.as_ptr(), exc.as_raw()) };
    }
}

// Placeholder mode: nothing ever reached `set`, so the slot is always
// empty and there is no root to drop. Present because `take` is reached
// from the dispatch bridge, which compiles on every target.
#[cfg(not(mruby_linked))]
fn unroot(_mrb: &beni::Mrb, _held: Option<Value>) {}

// SAFETY: identical argument to `crate::runtime::block_stack::BlockStack`
// — wasm32 is single-threaded inside any one Instance; the inner `Value`
// is `!Send + !Sync` but the surrounding Instance gives the same
// guarantee operationally. `static` requires `Sync` regardless.
unsafe impl Sync for RaisedBlock {}

/// The exception the most recent guest block raised, if it is still
/// waiting for the answer that says whether the Service rescued it.
pub(crate) static RAISED_BLOCK: RaisedBlock = RaisedBlock::new();
