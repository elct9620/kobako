//! The exception a guest block raised, held across the host round-trip.
//!
//! A block that raises does not end the invocation: the host re-raises at
//! the Service's `yield` site, and the Service may rescue it. Only
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
//! such as a refused block value.
//!
//! Between those two points the only answer the guest can hear is that
//! Service's own, so whatever is held belongs to the dispatch that takes
//! it — even when the block that raised was carried by an earlier Call.
//!
//! ## Cross-Sandbox isolation
//!
//! Same argument as `crate::runtime::block_stack`: each `Kobako::Sandbox`
//! owns its own `wasmtime::Instance` and therefore its own copy of this
//! module-level static.

use beni::sys::AsRawValue;
use beni::Value;

use core::cell::UnsafeCell;

/// Single-threaded interior-mutability slot for the exception the most
/// recent guest block raised.
pub(crate) struct RaisedBlock(UnsafeCell<Option<Value>>);

impl RaisedBlock {
    const fn new() -> Self {
        Self(UnsafeCell::new(None))
    }

    /// Record `exception` as what a block raised, rooting it against GC:
    /// the mruby frame that raised has already unwound by the time the
    /// host sees the failure, so nothing else keeps it alive.
    pub(crate) fn set(&self, mrb: &beni::Mrb, exception: Value) {
        self.clear(mrb);
        // SAFETY: see type doc.
        unsafe { *self.0.get() = Some(exception) };
        // SAFETY: `exception` is live on this VM; the paired unregister
        // runs in `take` or in the next `clear`.
        unsafe { beni::sys::mrb_gc_register(mrb.as_ptr(), exception.as_raw()) };
    }

    /// Release whatever is held, dropping its GC root.
    pub(crate) fn clear(&self, mrb: &beni::Mrb) {
        let _ = self.take(mrb);
    }

    /// Take the held exception, releasing its GC root. The caller either
    /// re-raises it — which roots it again through mruby's own exception
    /// slot — or drops it, having answered on an arm that is not a block's
    /// failure.
    pub(crate) fn take(&self, mrb: &beni::Mrb) -> Option<Value> {
        // SAFETY: see type doc.
        let held = unsafe { (*self.0.get()).take() }?;
        // SAFETY: paired with the register in `set`.
        unsafe { beni::sys::mrb_gc_unregister(mrb.as_ptr(), held.as_raw()) };
        Some(held)
    }
}

// SAFETY: identical argument to `crate::runtime::block_stack::BlockStack`
// — wasm32 is single-threaded inside any one Instance; the inner `Value`
// is `!Send + !Sync` but the surrounding Instance gives the same
// guarantee operationally. `static` requires `Sync` regardless.
unsafe impl Sync for RaisedBlock {}

/// The exception the most recent guest block raised, if it is still
/// waiting for the answer that says whether the Service rescued it.
pub(crate) static RAISED_BLOCK: RaisedBlock = RaisedBlock::new();
