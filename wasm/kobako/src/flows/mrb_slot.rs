//! Module-level static slot owning the live `Mrb` state.
//!
//! The slot carries the VM in canonical boot state (docs/behavior.md
//! B-49): populated at build time by the wizer bake
//! (`super::boot::bake_boot`) or lazily by the first entry on a
//! non-baked artifact. The block / yield mechanism (docs/behavior.md
//! B-23 → B-30) needs the *same* `mrb_state` to be reachable from
//! `__kobako_yield_to_block` while the original dispatch frame is
//! still on the wasm call stack, which is why the slot is a
//! module-level static rather than a stack local.
//!
//! ## Lifecycle contract
//!
//! 1. The bake (or the first entry's `boot_vm`) installs an opened,
//!    Kobako-initialised `Mrb` via `MRB.install`.
//! 2. Entry bodies borrow the live VM via `MRB.as_ref`.
//! 3. The slot is cleared only when a lazy boot fails mid-way; on
//!    every other path the VM stays installed — the host discards the
//!    whole instance after each invocation (ABI v2 per-invocation
//!    discipline), so `mrb_close` never needs to run.
//!
//! ## Cross-invocation isolation
//!
//! The host drives every invocation on a fresh instance of the module
//! (ABI v2), and `MRB` is a module-level static inside that instance's
//! wasm linear memory — two invocations see *different* memory
//! locations for this static, with no aliasing. The single-threaded
//! wasm execution model inside any one instance is what licenses the
//! `UnsafeCell` interior mutability here.

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

    /// Drop the held `Mrb` (if any) and leave the slot empty. Safe to
    /// call when the slot is already empty (no-op). After `clear`, any
    /// `&Mrb` borrow previously returned by `Self::as_ref` is dangling
    /// — callers must structure code so the borrow does not outlive the
    /// frame that owns the install/clear bracket.
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
