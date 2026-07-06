//! Per-invocation LIFO stack of guest-supplied blocks.
//!
//! When guest code calls `Service.method(...) { ... }`, the C-bridge
//! captures the block as a non-orphan `mrb_value` via the `"n*&"`
//! argspec and pushes it onto `BLOCK_STACK` before dispatching to the
//! host. The host's eventual `__kobako_yield_to_block` re-entry
//! reads `BLOCK_STACK.last()` to find the block bound to the active
//! dispatch frame. The push/pop pair is enforced by `BlockFrame`'s
//! drop guard so any bridge exit path — normal return, mruby raise,
//! Rust panic — preserves the LIFO invariant.
//!
//! The wire-level `block_given` bit is the observable shadow of
//! a push; the yield flow's read is the matching dereference.
//!
//! ## Cross-Sandbox isolation
//!
//! Same argument as `crate::flows::mrb_slot`: each `Kobako::Sandbox` owns
//! its own `wasmtime::Instance` and therefore its own copy of this
//! module-level static. The single-threaded wasm execution model
//! inside any one Instance licenses the `UnsafeCell` interior
//! mutability used here.

use beni::Value;

use core::cell::UnsafeCell;

/// Single-threaded interior-mutability stack of guest-supplied block
/// `mrb_value`s. Modelled after `crate::flows::mrb_slot::MrbSlot`
/// — the wasm Instance's single-threaded execution model is what
/// licenses the `UnsafeCell` interior mutation here.
pub(crate) struct BlockStack(UnsafeCell<Vec<Value>>);

impl BlockStack {
    const fn new() -> Self {
        Self(UnsafeCell::new(Vec::new()))
    }

    /// Push `block` onto the top of the stack.
    pub(crate) fn push(&self, block: Value) {
        // SAFETY: see type doc.
        unsafe { (*self.0.get()).push(block) };
    }

    /// Pop the top of the stack, discarding the value. Safe to call
    /// when the stack is empty (no-op).
    pub(crate) fn pop(&self) {
        // SAFETY: see type doc.
        unsafe {
            (*self.0.get()).pop();
        }
    }

    /// Return the topmost block, or `None` when the stack is empty.
    /// Consumed by `__kobako_yield_to_block` to identify the block
    /// bound to the active dispatch frame. The returned `Value`
    /// is a copy of the `mrb_value` stored on the stack — `Value` is
    /// `Copy` and the underlying `mrb_value` slot keeps the mruby GC
    /// rooting argument intact for the duration of the dispatch
    /// frame, so reading the top is safe inside the same single-
    /// threaded invocation that pushed it.
    #[cfg(mruby_linked)]
    pub(crate) fn last(&self) -> Option<Value> {
        // SAFETY: see type doc.
        unsafe { (*self.0.get()).last().copied() }
    }
}

// SAFETY: identical argument to `crate::flows::mrb_slot::MrbSlot` — wasm32
// is single-threaded inside any one Instance; the inner `Value` is
// `!Send + !Sync` but the surrounding Instance gives the same
// guarantee operationally. `static` requires `Sync` regardless.
unsafe impl Sync for BlockStack {}

/// Per-invocation LIFO stack of guest-supplied blocks.
pub(crate) static BLOCK_STACK: BlockStack = BlockStack::new();

/// RAII drop-guard that owns one push/pop pair on `BLOCK_STACK`.
/// Constructed via `BlockFrame::push_if_block` — the guard becomes a
/// no-op when the supplied `block` is `nil` (i.e. the caller did not
/// pass a block). Pop runs unconditionally on drop, mirroring the C
/// bridge's exit invariant: every entry must have a matching exit,
/// even on the mruby-raise / Rust-panic paths.
pub(crate) struct BlockFrame {
    active: bool,
}

impl BlockFrame {
    /// Push `block` onto `BLOCK_STACK` when it is non-nil and return
    /// a guard whose drop pops the same frame. When `block` is nil the
    /// guard is inert — `Drop` is a no-op.
    pub(crate) fn push_if_block(block: Value) -> Self {
        let active = !block.is_nil();
        if active {
            BLOCK_STACK.push(block);
        }
        Self { active }
    }
}

impl Drop for BlockFrame {
    fn drop(&mut self) {
        if self.active {
            BLOCK_STACK.pop();
        }
    }
}
