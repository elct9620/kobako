//! Module-level static slot owning the live `Mrb` in canonical boot state.
//!
//! It is a static rather than a stack local because
//! a `yield` downcall must reach the *same* `mrb_state` while the
//! dispatch frame that yielded is still on the wasm call stack. Once boot
//! succeeds it stays installed, since the host drops the whole instance
//! after each invocation.

use beni::Mrb;

use crate::runtime::instance_cell::InstanceCell;

/// The slot for the active `Mrb`, mutated from `&self` through an
/// `InstanceCell`. `install` / `clear` / `as_ref` are the only entry
/// points.
pub(super) struct MrbSlot(InstanceCell<Option<Mrb>>);

impl MrbSlot {
    const fn new() -> Self {
        Self(InstanceCell::new(None))
    }

    /// Install `mrb` into the slot, dropping any previously held value.
    /// The dropped `Mrb` runs its `mrb_close` automatically.
    ///
    /// # Safety
    ///
    /// No `&Mrb` borrow from `Self::as_ref` may be live.
    pub(super) unsafe fn install(&self, mrb: Mrb) {
        // SAFETY: the caller holds no borrow the write would invalidate.
        unsafe { *self.0.get() = Some(mrb) };
    }

    /// Drop the installed `Mrb`.
    ///
    /// # Safety
    ///
    /// No `&Mrb` borrow from `Self::as_ref` may be live, since each would
    /// dangle once the slot is cleared.
    pub(super) unsafe fn clear(&self) {
        // SAFETY: the caller holds no borrow the write would invalidate.
        unsafe { *self.0.get() = None };
    }

    /// Borrow the live `Mrb` if one is installed. The returned
    /// reference is valid until the next `Self::install` /
    /// `Self::clear` on this slot.
    #[inline]
    pub(super) fn as_ref(&self) -> Option<&Mrb> {
        // SAFETY: a write comes only through `install` / `clear`, whose
        // callers hold no borrow from here.
        unsafe { (*self.0.get()).as_ref() }
    }
}

/// The live `Mrb` slot. Installed by `super::boot::boot_vm` (directly
/// at the bake, or lazily on a non-baked artifact's first entry);
/// cleared only by `boot_vm`'s failure path.
pub(super) static MRB: MrbSlot = MrbSlot::new();
