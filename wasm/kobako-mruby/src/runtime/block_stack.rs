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

use beni::Proc;

use crate::runtime::instance_cell::InstanceCell;

/// The stack of guest-supplied blocks. Each method holds its borrow only
/// for its own body, so none outlives a write.
pub(crate) struct BlockStack(InstanceCell<Vec<Option<Proc>>>);

impl BlockStack {
    const fn new() -> Self {
        Self(InstanceCell::new(Vec::new()))
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
