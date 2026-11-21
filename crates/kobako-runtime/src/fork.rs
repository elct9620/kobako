//! What a forked child keeps of its parent, as Linux's `kernel/fork.c` copies
//! the memory but only the thread that called `fork`. A value made for the
//! process — a count of what its own threads hold, or a thread it started — is
//! the parent's alone, so the child starts from none.

use std::marker::PhantomData;
use std::ptr;
use std::sync::atomic::{AtomicPtr, AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};

/// How many forks this process descends through, as each child counts one
/// more in the handler `pthread_atfork` runs for it.
static GENERATION: AtomicU64 = AtomicU64::new(0);

/// The process a value was made in, as Linux's `struct pid` names a task
/// apart from any later one given its number: a forked child inherits the
/// value, but not having made it, and neither does a descendant that the
/// number came round to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Pid {
    number: u32,
    generation: u64,
}

impl Pid {
    /// This process.
    pub(crate) fn current() -> Self {
        count_forks();
        Self {
            number: std::process::id(),
            generation: GENERATION.load(Ordering::Relaxed),
        }
    }

    /// Whether this process is the one named.
    pub(crate) fn is_current(self) -> bool {
        self == Self::current()
    }

    /// Another process than this one, as a forked child sees its parent.
    #[cfg(test)]
    pub(crate) fn parent() -> Self {
        let current = Self::current();
        Self {
            number: current.number.wrapping_add(1),
            ..current
        }
    }
}

/// Count each fork from here on in the child, so a descendant given a dead
/// ancestor's number still tells itself apart. The handler only adds, which
/// is all a child may do before it returns from `fork`.
#[cfg(unix)]
fn count_forks() {
    static REGISTER: std::sync::Once = std::sync::Once::new();
    extern "C" fn forked() {
        GENERATION.fetch_add(1, Ordering::Relaxed);
    }
    REGISTER.call_once(|| {
        // SAFETY: `forked` touches one atomic, as a child handler may.
        let err = unsafe { libc::pthread_atfork(None, None, Some(forked)) };
        assert_eq!(err, 0, "pthread_atfork must take the fork handler");
    });
}

/// A process that cannot fork has no child to tell apart.
#[cfg(not(unix))]
fn count_forks() {}

/// A value of the process that made it, as `thread_local!` keeps one per
/// thread: a forked child, which inherits the memory but none of the parent's
/// other threads, sees none until it makes its own.
pub struct ProcessLocal<T> {
    slot: AtomicPtr<Slot<T>>,
    /// Shared across threads as each slot's lock is.
    _value: PhantomData<Mutex<Option<T>>>,
}

/// One process's value, behind a lock of its own.
struct Slot<T> {
    owner: Pid,
    value: Mutex<Option<T>>,
}

impl<T> ProcessLocal<T> {
    /// None made yet.
    pub const fn new() -> Self {
        Self {
            slot: AtomicPtr::new(ptr::null_mut()),
            _value: PhantomData,
        }
    }

    /// Run `f` on what this process holds, none when another process made
    /// what is held.
    pub fn with<R>(&self, f: impl FnOnce(&mut Option<T>) -> R) -> R {
        let mut value = self
            .own_slot()
            .value
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        f(&mut value)
    }

    /// This process's slot, put in place of another process's, which is left
    /// as it was: a thread that never came along may hold its lock, and its
    /// value may close what this process no longer owns, as a kqueue the
    /// fork did not copy.
    fn own_slot(&self) -> &Slot<T> {
        let current = Pid::current();
        let mut held = self.slot.load(Ordering::Acquire);
        loop {
            // SAFETY: a slot in place is freed only as the ProcessLocal drops.
            if let Some(slot) = unsafe { held.as_ref() } {
                if slot.owner == current {
                    return slot;
                }
            }
            let fresh = Box::into_raw(Box::new(Slot {
                owner: current,
                value: Mutex::new(None),
            }));
            match self
                .slot
                .compare_exchange(held, fresh, Ordering::AcqRel, Ordering::Acquire)
            {
                // SAFETY: `fresh` is in place, freed only as above.
                Ok(_) => return unsafe { &*fresh },
                Err(now) => {
                    // SAFETY: `fresh` never left this thread.
                    drop(unsafe { Box::from_raw(fresh) });
                    held = now;
                }
            }
        }
    }

    /// Stand in for a fork: what is held now was made by another process.
    #[cfg(test)]
    pub(crate) fn inherit(&self) {
        self.inherit_from(Pid::parent());
    }

    /// Stand in for a fork from `parent`. The slot this process made is left
    /// in place for any thread still holding it, as a fork leaves the
    /// parent's.
    #[cfg(test)]
    fn inherit_from(&self, parent: Pid) {
        let value = self.with(Option::take);
        let inherited = Box::into_raw(Box::new(Slot {
            owner: parent,
            value: Mutex::new(value),
        }));
        self.slot.store(inherited, Ordering::Release);
    }
}

impl<T> Default for ProcessLocal<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Drop for ProcessLocal<T> {
    fn drop(&mut self) {
        let held = *self.slot.get_mut();
        // SAFETY: a slot in place is freed only here.
        if unsafe { held.as_ref() }.is_some_and(|slot| slot.owner.is_current()) {
            // SAFETY: as above, and nothing borrows `self` any more.
            drop(unsafe { Box::from_raw(held) });
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;
    use std::sync::{Arc, Barrier};
    use std::thread;

    use super::*;

    /// Counts each drop of itself.
    struct Dropped(Arc<AtomicUsize>);

    impl Drop for Dropped {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

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

    #[test]
    fn a_value_an_ancestor_under_the_same_number_made_is_not_handed_out() {
        let local = ProcessLocal::new();
        local.with(|held| *held = Some(1));
        let current = Pid::current();
        local.inherit_from(Pid {
            generation: current.generation.wrapping_sub(1),
            ..current
        });
        assert_eq!(
            local.with(|held| *held),
            None,
            "a ProcessLocal must hand none of what an ancestor made to a descendant that its \
             process number came round to"
        );
    }

    #[test]
    fn a_value_made_in_another_process_is_never_dropped() {
        let drops = Arc::new(AtomicUsize::new(0));
        let local = ProcessLocal::new();
        local.with(|held| *held = Some(Dropped(drops.clone())));
        local.inherit();
        local.with(|held| *held = Some(Dropped(Arc::new(AtomicUsize::new(0)))));
        drop(local);
        assert_eq!(
            drops.load(Ordering::SeqCst),
            0,
            "a ProcessLocal must never drop what another process made, which may close what \
             the forked child no longer owns"
        );
    }

    #[test]
    fn a_value_made_in_this_process_drops_with_it() {
        let drops = Arc::new(AtomicUsize::new(0));
        let local = ProcessLocal::new();
        local.with(|held| *held = Some(Dropped(drops.clone())));
        drop(local);
        assert_eq!(
            drops.load(Ordering::SeqCst),
            1,
            "a ProcessLocal must drop what this process made as it drops"
        );
    }

    #[test]
    fn threads_replacing_an_inherited_value_at_once_share_one() {
        const THREADS: usize = 8;
        let local = Arc::new(ProcessLocal::<usize>::new());
        local.with(|held| *held = Some(100));
        local.inherit();
        let start = Arc::new(Barrier::new(THREADS));
        let threads: Vec<_> = (0..THREADS)
            .map(|_| {
                let (local, start) = (local.clone(), start.clone());
                thread::spawn(move || {
                    start.wait();
                    local.with(|held| *held.get_or_insert(0) += 1);
                })
            })
            .collect();
        threads.into_iter().for_each(|t| t.join().unwrap());
        assert_eq!(
            local.with(|held| *held),
            Some(THREADS),
            "threads of a forked child replacing an inherited value at once must all count on \
             the one value this process puts in place"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_forked_child_counts_one_generation_more() {
        let parent = Pid::current();
        let mut ends = [0; 2];
        // SAFETY: a pipe into two descriptors this test owns.
        assert_eq!(unsafe { libc::pipe(ends.as_mut_ptr()) }, 0, "a pipe opens");
        // SAFETY: the child only reads an atomic, writes, and exits.
        let child = unsafe { libc::fork() };
        if child == 0 {
            let generation = Pid::current().generation.to_ne_bytes();
            // SAFETY: the child writes its generation and exits at once.
            unsafe {
                libc::write(ends[1], generation.as_ptr().cast(), generation.len());
                libc::_exit(0);
            }
        }
        let mut read = [0u8; 8];
        // SAFETY: reading into a buffer this test owns, then reaping the child.
        unsafe {
            libc::close(ends[1]);
            libc::read(ends[0], read.as_mut_ptr().cast(), read.len());
            libc::close(ends[0]);
            libc::waitpid(child, ptr::null_mut(), 0);
        }
        assert_eq!(
            u64::from_ne_bytes(read),
            parent.generation + 1,
            "a child forked after kobako named its process must count one fork more than its \
             parent, so a descendant given the parent's number tells itself apart"
        );
    }
}
