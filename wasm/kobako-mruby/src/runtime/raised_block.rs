//! The exception a guest block raised, held across the host round-trip.
//!
//! A block that raises does not end the invocation: the host re-raises at
//! the Service's `yield` site, and the Service may rescue it (B-24). Only
//! an unrescued one comes back, and when it does the failure is the
//! caller's own — so the caller re-raises the exception it raised rather
//! than a reconstruction of it, which is what the same two frames would
//! do with no Sandbox between them.
//!
//! The wait ends the moment the guest runs again for any other reason:
//! `crate::runtime::bridges` takes on every dispatch answer, whatever the
//! arm, and `crate::flows::yield_block` clears on every re-entry, since a
//! Service reaching a second yield continued past the first failure —
//! which is what rescuing it means. Left behind past either point, a spent
//! exception would be handed to a later failure that has none of its own,
//! such as a refused block value (E-21, E-22).
//!
//! The block is held alongside the exception and only its own dispatch may
//! take it. With both rules above in force nothing reaches the slot out of
//! turn, so the check is a floor: a mismatch degrades to the ordinary
//! fault path rather than continuing the wrong exception.
//!
//! ## Cross-Sandbox isolation
//!
//! Same argument as `crate::runtime::block_stack`: each `Kobako::Sandbox`
//! owns its own `wasmtime::Instance` and therefore its own copy of this
//! module-level static.

use beni::Value;

use core::cell::UnsafeCell;

/// One block's failure, waiting for the answer that says whether the
/// Service rescued it.
struct Held {
    block: Value,
    exception: Value,
}

/// Single-threaded interior-mutability slot for the exception the most
/// recent guest block raised.
pub(crate) struct RaisedBlock(UnsafeCell<Option<Held>>);

impl RaisedBlock {
    const fn new() -> Self {
        Self(UnsafeCell::new(None))
    }

    /// Record `exception` as what `block` raised, rooting it against GC:
    /// the mruby frame that raised has already unwound by the time the
    /// host sees the failure, so nothing else keeps it alive.
    #[cfg(mruby_linked)]
    pub(crate) fn set(&self, mrb: &beni::Mrb, block: Value, exception: Value) {
        self.clear(mrb);
        // SAFETY: see type doc.
        unsafe { *self.0.get() = Some(Held { block, exception }) };
        // SAFETY: `exception` is live on this VM; the paired unregister
        // runs in `take_for` or in the next `clear`.
        unsafe { beni::sys::mrb_gc_register(mrb.as_ptr(), exception.as_raw()) };
    }

    /// Release whatever is held, dropping its GC root.
    #[cfg(mruby_linked)]
    pub(crate) fn clear(&self, mrb: &beni::Mrb) {
        // SAFETY: see type doc.
        let held = unsafe { (*self.0.get()).take() };
        unroot(mrb, held);
    }

    /// Take the exception held for `block`, releasing the GC root, or
    /// `None` when what is held belongs to another block. The caller
    /// either re-raises it — which roots it again through mruby's own
    /// exception slot — or drops it, having answered on an arm that is
    /// not the block's failure.
    pub(crate) fn take_for(&self, mrb: &beni::Mrb, block: Value) -> Option<Value> {
        // SAFETY: see type doc.
        let slot = unsafe { &mut *self.0.get() };
        if !slot.as_ref()?.block.obj_equal(mrb, block) {
            return None;
        }
        let exception = slot.as_ref()?.exception;
        unroot(mrb, slot.take());
        Some(exception)
    }
}

#[cfg(mruby_linked)]
fn unroot(mrb: &beni::Mrb, held: Option<Held>) {
    if let Some(one) = held {
        // SAFETY: paired with the register in `set`.
        unsafe { beni::sys::mrb_gc_unregister(mrb.as_ptr(), one.exception.as_raw()) };
    }
}

// Placeholder mode: nothing ever reached `set`, so the slot is always
// empty and there is no root to drop. Present because `take_for` is
// reached from the dispatch bridge, which compiles on every target.
#[cfg(not(mruby_linked))]
fn unroot(_mrb: &beni::Mrb, _held: Option<Held>) {}

// SAFETY: identical argument to `crate::runtime::block_stack::BlockStack`
// — wasm32 is single-threaded inside any one Instance; the inner `Value`
// is `!Send + !Sync` but the surrounding Instance gives the same
// guarantee operationally. `static` requires `Sync` regardless.
unsafe impl Sync for RaisedBlock {}

/// The exception the most recent guest block raised, if it is still
/// waiting for the answer that says whether the Service rescued it.
pub(crate) static RAISED_BLOCK: RaisedBlock = RaisedBlock::new();
