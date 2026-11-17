//! What a forked child keeps of its parent, as Linux's `kernel/fork.c` copies
//! the memory but only the thread that called `fork`. A value made for the
//! process — a count of what its own threads hold, or a thread it started — is
//! the parent's alone, so the child starts from none.

use std::sync::{Mutex, PoisonError};

/// A value of the process that made it, as `thread_local!` keeps one per
/// thread: a forked child, which inherits the memory but none of the parent's
/// other threads, sees none until it makes its own.
pub struct ProcessLocal<T> {
    slot: Mutex<(u32, Option<T>)>,
}

impl<T> ProcessLocal<T> {
    /// None made yet.
    pub const fn new() -> Self {
        Self {
            slot: Mutex::new((0, None)),
        }
    }

    /// Run `f` on what this process holds, emptied first when another process
    /// made it.
    pub fn with<R>(&self, f: impl FnOnce(&mut Option<T>) -> R) -> R {
        let mut slot = self.slot.lock().unwrap_or_else(PoisonError::into_inner);
        let pid = std::process::id();
        if slot.0 != pid {
            *slot = (pid, None);
        }
        f(&mut slot.1)
    }

    /// Stand in for a fork: what is held now was made by another process.
    #[cfg(test)]
    pub(crate) fn inherit(&self) {
        let mut slot = self.slot.lock().unwrap_or_else(PoisonError::into_inner);
        slot.0 = std::process::id().wrapping_add(1);
    }
}

impl<T> Default for ProcessLocal<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_value_made_in_another_process_is_not_handed_out() {
        let local = ProcessLocal::new();
        local.with(|held| *held = Some(1));
        local.inherit();
        assert_eq!(
            local.with(|held| *held),
            None,
            "a ProcessLocal must hand a forked child none of what its parent made"
        );
    }

    #[test]
    fn a_value_made_in_this_process_is_kept() {
        let local = ProcessLocal::new();
        local.with(|held| *held = Some(1));
        assert_eq!(
            local.with(|held| *held),
            Some(1),
            "a ProcessLocal must keep what this process made"
        );
    }
}
