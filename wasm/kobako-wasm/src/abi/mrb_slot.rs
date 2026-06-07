//! Per-invocation static slot owning the active `Mrb` state.
//!
//! Each `__kobako_eval` / `__kobako_run` invocation opens a fresh
//! `mrb_state` via `Mrb::open()`. Previously the value lived as a stack
//! local in `eval_body` / `run_body` and dropped automatically at
//! function return. The block / yield mechanism (docs/behavior.md B-23
//! → B-30) needs the *same* `mrb_state` to be
//! reachable from `__kobako_yield_to_block` while the original dispatch
//! frame is still on the wasm call stack, so the slot is lifted from
//! stack-let into a module-level static here.
//!
//! ## Lifecycle contract
//!
//! 1. `__kobako_eval` / `__kobako_run` install a freshly opened `Mrb`
//!    via `MRB.install`.
//! 2. The function body borrows the live VM via `MRB.as_ref`.
//! 3. Every exit path (success, panic outcome, boot error) clears the
//!    slot via `MRB.clear`, which drops the held `Mrb` and runs the
//!    existing `mrb_close` glue automatically.
//!
//! Step 3 is enforced structurally by `MrbScope` — a drop-guard that
//! the entry-point body declares immediately, so any early `return`
//! along the panic / error paths still clears the slot.
//!
//! ## Cross-Sandbox isolation
//!
//! Each `Kobako::Sandbox` owns its own `wasmtime::Instance`, and
//! `MRB` is a module-level static inside that Instance's wasm linear
//! memory — two Sandboxes on two OS Threads see *different* memory
//! locations for this static, with no aliasing. The single-threaded
//! wasm execution model inside any one Instance is what licenses the
//! `UnsafeCell` interior mutability here.

#[cfg(target_arch = "wasm32")]
use crate::mruby::Mrb;

#[cfg(target_arch = "wasm32")]
use core::cell::UnsafeCell;

/// Single-threaded interior-mutability slot for the active `Mrb` — an
/// `UnsafeCell<Option<Mrb>>` that the single-threaded wasm execution
/// model permits us to mutate from `&self`. `install` / `clear` /
/// `as_ref` are the only entry points; aliasing rules are documented at
/// the call sites in the `MrbScope` doc and the lifecycle section
/// above.
#[cfg(target_arch = "wasm32")]
pub(super) struct MrbSlot(UnsafeCell<Option<Mrb>>);

#[cfg(target_arch = "wasm32")]
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
    /// Frame-shaped use (install at entry, borrow inside body, clear at
    /// exit via `MrbScope`) satisfies this naturally.
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
#[cfg(target_arch = "wasm32")]
unsafe impl Sync for MrbSlot {}

/// The active per-invocation `Mrb` slot. Installed by
/// `super::boot::open_with_preamble`; cleared by `MrbScope`'s drop.
#[cfg(target_arch = "wasm32")]
pub(super) static MRB: MrbSlot = MrbSlot::new();

/// Drop-guard that clears `MRB` when the enclosing scope returns.
/// Declare it once at the top of every entry-point body so panic-outcome
/// `return` branches still clear the slot. Calling `clear` on an empty
/// slot is a no-op, so the guard is safe to declare before the install
/// even succeeds.
#[cfg(target_arch = "wasm32")]
pub(super) struct MrbScope;

#[cfg(target_arch = "wasm32")]
impl Drop for MrbScope {
    fn drop(&mut self) {
        MRB.clear();
    }
}
