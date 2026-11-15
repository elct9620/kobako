//! The cell a module-level static mutates through, once per Wasm instance.
//!
//! Each invocation runs on a fresh instance, so a static never carries
//! state from one invocation into the next, and an instance runs one
//! thread, so no two threads reach the cell. Those two facts make the
//! interior mutation sound; a `static` still has to be `Sync`, which the
//! cell claims on their strength for whatever it holds.

use core::cell::UnsafeCell;

pub struct InstanceCell<T>(UnsafeCell<T>);

impl<T> InstanceCell<T> {
    pub const fn new(value: T) -> Self {
        Self(UnsafeCell::new(value))
    }

    /// A pointer to the held value. A write through it while a borrow read
    /// through it is live is undefined behavior, as with `UnsafeCell::get`.
    pub fn get(&self) -> *mut T {
        self.0.get()
    }
}

// SAFETY: an instance runs one thread and each invocation has its own
// instance, so the cell is never reached from two threads; see the module
// doc.
unsafe impl<T> Sync for InstanceCell<T> {}
