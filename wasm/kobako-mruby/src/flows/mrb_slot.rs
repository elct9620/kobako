//! Module-level static slot owning the live `Mrb` in canonical boot state.
//!
//! It is a static rather than a stack local because
//! `__kobako_yield_to_block` must reach the *same* `mrb_state` while the
//! dispatch frame that yielded is still on the wasm call stack. Once boot
//! succeeds it stays installed, since the host drops the whole instance
//! after each invocation.
//!
//! Each invocation runs on a fresh instance, so the static never aliases
//! across invocations, and wasm runs single-threaded inside one instance;
//! together they license the `UnsafeCell` here.

use beni::Mrb;

use core::cell::UnsafeCell;

/// Single-threaded interior-mutability slot for the active `Mrb` — an
/// `UnsafeCell<Option<Mrb>>` that the single-threaded wasm execution
/// model permits us to mutate from `&self`. `install` / `clear` /
/// `as_ref` are the only entry points; aliasing rules live in the
/// lifecycle section above.
pub(super) struct MrbSlot(UnsafeCell<Option<Mrb>>);

impl MrbSlot {
    const fn new() -> Self {
        Self(UnsafeCell::new(None))
    }

    /// Install `mrb` into the slot, dropping any previously held value.
    /// The dropped `Mrb` runs its `mrb_close` automatically.
    ///
    /// # Safety contract
    ///
    /// No outstanding `&Mrb` borrow from `Self::as_ref` may be live.
    /// Boot-shaped use (install once per instance, before any entry
    /// body borrows) satisfies this naturally.
    pub(super) fn install(&self, mrb: Mrb) {
        // SAFETY: see type doc — single-threaded wasm execution + the
        // lifecycle contract documented in this module's header.
        unsafe { *self.0.get() = Some(mrb) };
    }

    /// After `clear`, any `&Mrb` borrow previously returned by
    /// `Self::as_ref` is dangling, so the borrow must not outlive the frame
    /// that owns the install/clear bracket.
    pub(super) fn clear(&self) {
        // SAFETY: see type doc — `clear` runs at frame exit, after all
        // body-scoped `&Mrb` borrows from `as_ref` have ended.
        unsafe { *self.0.get() = None };
    }

    /// Borrow the live `Mrb` if one is installed. The returned
    /// reference is valid until the next `Self::install` /
    /// `Self::clear` on this slot.
    #[inline]
    pub(super) fn as_ref(&self) -> Option<&Mrb> {
        // SAFETY: see type doc.
        unsafe { (*self.0.get()).as_ref() }
    }
}

// SAFETY: wasm32 is single-threaded; the slot is never observed from
// more than one logical owner at a time inside a wasm instance. The
// inner `Mrb` is `!Send + !Sync` to forbid cross-thread movement at the
// type level, but the surrounding wasm Instance gives us the same
// guarantee operationally. `static` requires `Sync` regardless.
unsafe impl Sync for MrbSlot {}

/// The live `Mrb` slot. Installed by `super::boot::boot_vm` (directly
/// at the bake, or lazily on a non-baked artifact's first entry);
/// cleared only by `boot_vm`'s failure path.
pub(super) static MRB: MrbSlot = MrbSlot::new();
