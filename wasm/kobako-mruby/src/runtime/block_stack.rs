//! Per-invocation LIFO stack holding one slot for each open Call: the
//! block it carried, or none.
//!
//! When guest code calls `Service.method(...) { ... }`, the C-bridge reads
//! the block off its call frame and `crate::dispatch` parks it here for the
//! host round-trip. A `yield` downcall names the open Call by its slot,
//! counted from the outermost as the host counts the Calls it answers, so
//! a block runs as itself even from inside a later Call.
//!
//! The wire-level `block_given` bit is read off the guard rather than
//! computed beside it, so a parked block and the bit announcing it are
//! one statement; the yield flow's read is the matching dereference.
//!
//! ## Cross-Sandbox isolation
//!
//! Same argument as `crate::flows::mrb_slot`: each `Kobako::Sandbox` owns
//! its own `wasmtime::Instance` and therefore its own copy of this
//! module-level static. The single-threaded wasm execution model
//! inside any one Instance licenses the `UnsafeCell` interior
//! mutability used here.

use beni::Proc;

use core::cell::UnsafeCell;

/// Single-threaded interior-mutability stack of guest-supplied blocks.
/// Modelled after `crate::flows::mrb_slot::MrbSlot`
/// — the wasm Instance's single-threaded execution model is what
/// licenses the `UnsafeCell` interior mutation here.
pub(crate) struct BlockStack(UnsafeCell<Vec<Option<Proc>>>);

impl BlockStack {
    const fn new() -> Self {
        Self(UnsafeCell::new(Vec::new()))
    }

    pub(crate) fn push(&self, block: Option<Proc>) {
        // SAFETY: see type doc.
        unsafe { (*self.0.get()).push(block) };
    }

    pub(crate) fn pop(&self) {
        // SAFETY: see type doc.
        unsafe {
            (*self.0.get()).pop();
        }
    }

    /// The block of the open Call `frame` counts to, if that Call is open
    /// and carried one. The Call's own frame on the mruby stack keeps the
    /// block alive while it is open, so a copy is safe to use inside the
    /// same single-threaded invocation that pushed it.
    pub(crate) fn get(&self, frame: u32) -> Option<Proc> {
        // SAFETY: see type doc.
        let slots = unsafe { &*self.0.get() };
        slots.get(frame as usize).copied().flatten()
    }
}

// SAFETY: identical argument to `crate::flows::mrb_slot::MrbSlot` — wasm32
// is single-threaded inside any one Instance; the inner handle is
// `!Send + !Sync` but the surrounding Instance gives the same
// guarantee operationally. `static` requires `Sync` regardless.
unsafe impl Sync for BlockStack {}

/// Per-invocation LIFO stack of the open Calls' blocks.
pub(crate) static BLOCK_STACK: BlockStack = BlockStack::new();

/// RAII drop-guard that owns one push/pop pair on `BLOCK_STACK`.
/// Constructed via `BlockFrame::park` for every Call, block or none.
/// Drop pops the slot when the dispatch bridge frame returns, so its
/// several return points need no manual pop.
///
/// Internal because holding it is only half of taking a block, and the
/// other half is a bit on the wire: `crate::dispatch` owns both so the
/// two cannot disagree.
pub(crate) struct BlockFrame {
    block_given: bool,
}

impl BlockFrame {
    /// Push this Call's slot onto `BLOCK_STACK` and return a guard whose
    /// drop pops it.
    pub(crate) fn park(block: Option<Proc>) -> Self {
        let block_given = block.is_some();
        BLOCK_STACK.push(block);
        Self { block_given }
    }

    /// Whether this frame parked a block — the wire's `block_given` bit,
    /// read off the guard so the two cannot be stated differently.
    pub(crate) fn block_given(&self) -> bool {
        self.block_given
    }
}

impl Drop for BlockFrame {
    fn drop(&mut self) {
        BLOCK_STACK.pop();
    }
}
