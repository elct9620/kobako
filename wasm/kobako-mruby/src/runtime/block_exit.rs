//! How a guest block left, held across the host round-trip.
//!
//! A block that raises or breaks does not end the invocation where it
//! stops: the host unwinds or re-raises at the Service's `yield` site, and
//! only then does the answer come back. When it does, the caller continues
//! the exit the block took rather than a reconstruction of it, which is
//! what the same frames would do with no Sandbox between them.
//!
//! A raise waits only while the Service may still rescue it. The wait ends
//! the moment the guest runs again for any other reason:
//! `crate::runtime::bridges` takes on every dispatch answer, whatever the
//! arm, and `crate::flows::yield_block` clears on every re-entry, since a
//! Service reaching a second yield continued past the first failure —
//! which is what rescuing it means. Left behind past either point, a spent
//! exception would be handed to a later failure that has none of its own,
//! such as a refused block value.
//!
//! A `break` or `return` cannot be rescued, only unwound through, so it
//! waits for the break answer alone. A Service unwinding through its
//! `ensure` may yield again; that block running to its end leaves the
//! held exit in place, and one exiting again replaces it, as in Ruby.
//!
//! ## Cross-Sandbox isolation
//!
//! Same argument as `crate::runtime::block_stack`: each `Kobako::Sandbox`
//! owns its own `wasmtime::Instance` and therefore its own copy of these
//! module-level statics.

use beni::sys::AsRawValue;
use beni::Value;

use core::cell::UnsafeCell;

/// Single-threaded interior-mutability slot for the object a guest block
/// left with.
pub(crate) struct HeldExit(UnsafeCell<Option<Value>>);

impl HeldExit {
    const fn new() -> Self {
        Self(UnsafeCell::new(None))
    }

    /// Hold `exit`, rooting it against GC: the mruby frame it left has
    /// already unwound by the time the host answers, so nothing else keeps
    /// it alive.
    pub(crate) fn set(&self, mrb: &beni::Mrb, exit: Value) {
        self.clear(mrb);
        // SAFETY: see type doc.
        unsafe { *self.0.get() = Some(exit) };
        // SAFETY: `exit` is live on this VM; the paired unregister runs in
        // `take` or in the next `clear`.
        unsafe { beni::sys::mrb_gc_register(mrb.as_ptr(), exit.as_raw()) };
    }

    /// Release whatever is held, dropping its GC root.
    pub(crate) fn clear(&self, mrb: &beni::Mrb) {
        let _ = self.take(mrb);
    }

    /// Take the held exit, releasing its GC root. The caller either
    /// re-raises it — which roots it again through mruby's own exception
    /// slot — or drops it, having answered on an arm that does not continue
    /// it.
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
unsafe impl Sync for HeldExit {}

/// The exception the most recent guest block raised, if it is still
/// waiting for the answer that says whether the Service rescued it.
pub(crate) static RAISED_BLOCK: HeldExit = HeldExit::new();

/// The `break` or `return` the most recent guest block left with, waiting
/// for the Service it unwinds to answer.
pub(crate) static BROKEN_BLOCK: HeldExit = HeldExit::new();
