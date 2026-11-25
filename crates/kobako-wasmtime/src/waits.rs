//! How many waits a test's own thread makes. A process registers one
//! Blocking, so every test here shares the one this module registers, and
//! each counts only the waits of the thread that asked.

use std::cell::RefCell;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Once};
use std::time::{Duration, Instant};

use kobako_runtime::sched::{register_blocking, Blocking};

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

/// The longest a test waits for another thread, so what never arrives fails
/// the test instead of hanging the suite.
pub(crate) const MAX_WAIT: Duration = Duration::from_secs(10);

/// Wait until `arrived`, failing the test with `what` past `MAX_WAIT`.
pub(crate) fn wait_until(what: &str, mut arrived: impl FnMut() -> bool) {
    let started = Instant::now();
    while !arrived() {
        assert!(
            started.elapsed() < MAX_WAIT,
            "{what} did not happen within {MAX_WAIT:?}"
        );
        std::thread::yield_now();
    }
}

/// Counts each wait this thread makes from now on, starting at zero.
pub(crate) fn count_waits() -> Arc<AtomicUsize> {
    static REGISTER: Once = Once::new();
    REGISTER.call_once(|| {
        assert!(
            register_blocking(Box::new(CountsWaits)).is_ok(),
            "another Blocking was registered first; register only this one in the crate's tests"
        );
    });
    let counts = Arc::<AtomicUsize>::default();
    COUNTS.with(|held| *held.borrow_mut() = Some(counts.clone()));
    counts
}
