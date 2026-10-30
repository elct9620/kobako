//! Who is running and how it waits, beneath every lock, as Linux's
//! `kernel/sched` is: a lock asks only for its wait to happen, so nothing
//! above it knows what waiting means to the frontend.

use std::sync::OnceLock;
use std::thread::{self, ThreadId};

/// How a thread waits for a lock another thread holds. A frontend whose
/// threads share a lock of their own, as Ruby's threads share the GVL,
/// gives that lock up for the wait, or a holder that needs it to finish
/// never could.
pub trait Blocking: Send + Sync {
    /// Run `wait`, which returns once the lock is taken.
    fn wait(&self, wait: &mut dyn FnMut());
}

static BLOCKING: OnceLock<Box<dyn Blocking>> = OnceLock::new();

/// Have every thread of this process wait through `blocking`, as a frontend
/// says once at startup; a later one is handed back, as
/// `register_filesystem` refuses a second. Without one a thread just waits.
pub fn register_blocking(blocking: Box<dyn Blocking>) -> Result<(), Box<dyn Blocking>> {
    BLOCKING.set(blocking)
}

/// Run `wait` as this process's threads wait, as a lock's slow path calls
/// `schedule`.
pub(crate) fn schedule(wait: &mut dyn FnMut()) {
    match BLOCKING.get() {
        Some(blocking) => blocking.wait(wait),
        None => wait(),
    }
}

thread_local! {
    static CURRENT: ThreadId = thread::current().id();
}

/// Who is running, as Linux's `current`.
pub(crate) fn current() -> ThreadId {
    CURRENT.with(|current| *current)
}

/// A process has one Blocking, so a test counts only the waits of the thread
/// that asked to.
#[cfg(test)]
pub(crate) mod testing {
    use std::cell::RefCell;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use super::{Blocking, BLOCKING};

    thread_local! {
        static COUNTS: RefCell<Option<Arc<AtomicUsize>>> = const { RefCell::new(None) };
    }

    struct CountsWaits;

    impl Blocking for CountsWaits {
        fn wait(&self, wait: &mut dyn FnMut()) {
            COUNTS.with(|counts| {
                if let Some(counts) = counts.borrow().as_ref() {
                    counts.fetch_add(1, Ordering::SeqCst);
                }
            });
            wait();
        }
    }

    /// Counts each wait this thread makes through the registered Blocking.
    pub(crate) fn count_waits() -> Arc<AtomicUsize> {
        BLOCKING.get_or_init(|| Box::new(CountsWaits));
        let counts = Arc::<AtomicUsize>::default();
        COUNTS.with(|held| *held.borrow_mut() = Some(counts.clone()));
        counts
    }
}
