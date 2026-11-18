//! What a forked child keeps of its parent, as Linux's `kernel/fork.c` copies
//! the memory but only the thread that called `fork`. A value made for the
//! process — a count of what its own threads hold, or a thread it started — is
//! the parent's alone, so the child starts from none.

use std::sync::{Mutex, PoisonError};

/// The process a value was made in, as Linux's `struct pid` names a task: a
/// forked child inherits the value, but not having made it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Pid(u32);

impl Pid {
    /// This process.
    pub(crate) fn current() -> Self {
        Self(std::process::id())
    }

    /// Whether this process is the one named.
    pub(crate) fn is_current(self) -> bool {
        self == Self::current()
    }

    /// Another process than this one, as a forked child sees its parent.
    #[cfg(test)]
    pub(crate) fn parent() -> Self {
        Self(std::process::id().wrapping_add(1))
    }
}

/// A value of the process that made it, as `thread_local!` keeps one per
/// thread: a forked child, which inherits the memory but none of the parent's
/// other threads, sees none until it makes its own.
pub struct ProcessLocal<T> {
    slot: Mutex<(Option<Pid>, Option<T>)>,
}

impl<T> ProcessLocal<T> {
    /// None made yet.
    pub const fn new() -> Self {
        Self {
            slot: Mutex::new((None, None)),
        }
    }

    /// Run `f` on what this process holds, emptied first when another process
    /// made it.
    pub fn with<R>(&self, f: impl FnOnce(&mut Option<T>) -> R) -> R {
        let mut slot = self.slot.lock().unwrap_or_else(PoisonError::into_inner);
        if !slot.0.is_some_and(Pid::is_current) {
            *slot = (Some(Pid::current()), None);
        }
        f(&mut slot.1)
    }

    /// Stand in for a fork: what is held now was made by another process.
    #[cfg(test)]
    pub(crate) fn inherit(&self) {
        let mut slot = self.slot.lock().unwrap_or_else(PoisonError::into_inner);
        slot.0 = Some(Pid::parent());
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
