//! Per-invocation LIFO stack of guest-supplied blocks.
//!
//! When guest code calls `Service.method(...) { ... }`, the C-bridge
//! captures the block as a non-orphan `mrb_value` via the `"n*&"`
//! argspec and pushes it onto `BLOCK_STACK` before dispatching to the
//! host. The host's eventual `__kobako_yield_to_block` re-entry (S5+)
//! reads `BLOCK_STACK.last()` to find the block bound to the active
//! dispatch frame. The push/pop pair is enforced by `BlockFrame`'s
//! drop guard so any bridge exit path — normal return, mruby raise,
//! Rust panic — preserves the LIFO invariant.
//!
//! No reader exists in S3; the stack is established here so the wire-
//! level `block_given` bit (B-23) lines up with a real reference the
//! S4+ yield export can dereference.
//!
//! ## Cross-Sandbox isolation
//!
//! Same argument as `super::mrb_slot`: each `Kobako::Sandbox` owns
//! its own `wasmtime::Instance` and therefore its own copy of this
//! module-level static. The single-threaded wasm execution model
//! inside any one Instance licenses the `UnsafeCell` interior
//! mutability used here.

#[cfg(target_arch = "wasm32")]
use crate::mruby::sys::Value;

#[cfg(target_arch = "wasm32")]
use core::cell::UnsafeCell;

/// Single-threaded interior-mutability stack of guest-supplied block
/// `mrb_value`s. Modelled after `super::outcome_buffer::OutcomeBuffer`
/// — the wasm Instance's single-threaded execution model is what
/// licenses the `UnsafeCell` interior mutation here.
#[cfg(target_arch = "wasm32")]
pub(crate) struct BlockStack(UnsafeCell<Vec<Value>>);

#[cfg(target_arch = "wasm32")]
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
    /// bound to the active dispatch frame (B-24). The returned `Value`
    /// is a copy of the `mrb_value` stored on the stack — `Value` is
    /// `Copy` and the underlying `mrb_value` slot keeps the mruby GC
    /// rooting argument intact for the duration of the dispatch
    /// frame, so reading the top is safe inside the same single-
    /// threaded invocation that pushed it.
    pub(crate) fn last(&self) -> Option<Value> {
        // SAFETY: see type doc.
        unsafe { (*self.0.get()).last().copied() }
    }
}

// SAFETY: identical argument to `super::mrb_slot::MrbSlot` — wasm32
// is single-threaded inside any one Instance; the inner `Value` is
// `!Send + !Sync` but the surrounding Instance gives the same
// guarantee operationally. `static` requires `Sync` regardless.
#[cfg(target_arch = "wasm32")]
unsafe impl Sync for BlockStack {}

/// Per-invocation LIFO stack of guest-supplied blocks.
#[cfg(target_arch = "wasm32")]
pub(crate) static BLOCK_STACK: BlockStack = BlockStack::new();

/// RAII drop-guard that owns one push/pop pair on `BLOCK_STACK`.
/// Constructed via `BlockFrame::push_if_block` — the guard becomes a
/// no-op when the supplied `block` is `nil` (i.e. the caller did not
/// pass a block). Pop runs unconditionally on drop, mirroring the C
/// bridge's exit invariant: every entry must have a matching exit,
/// even on the mruby-raise / Rust-panic paths.
#[cfg(target_arch = "wasm32")]
pub(crate) struct BlockFrame {
    active: bool,
}

#[cfg(target_arch = "wasm32")]
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

#[cfg(target_arch = "wasm32")]
impl Drop for BlockFrame {
    fn drop(&mut self) {
        if self.active {
            BLOCK_STACK.pop();
        }
    }
}
