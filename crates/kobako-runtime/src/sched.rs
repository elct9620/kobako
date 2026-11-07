//! Who is running and how it waits, beneath every lock, as Linux's
//! `kernel/sched` is: a lock asks only for its wait to happen, so nothing
//! above it knows what waiting means to the frontend. A run past its deadline,
//! or one its frontend interrupted, stops waiting, as a fatal signal ends
//! Linux's killable waits, so one Sandbox holding a lock never holds another's
//! thread past that one's own end.

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread::{self, ThreadId};
use std::time::{Duration, Instant};

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

/// A thread as its frontend may interrupt it from another thread, as a signal
/// reaches a Linux task.
#[derive(Clone, Default)]
pub struct Task {
    interrupted: Arc<AtomicBool>,
}

impl Task {
    /// The calling thread.
    pub fn current() -> Self {
        TASK.with(|task| task.borrow().task.clone())
    }

    /// Have every wait of the run this thread carries out stop, as a fatal
    /// signal ends a killable wait; it holds until that run ends.
    pub fn interrupt(&self) {
        self.interrupted.store(true, Ordering::SeqCst);
    }
}

/// The task this thread carries out, as Linux's `current`, and when its run is
/// to stop.
#[derive(Default)]
struct Current {
    task: Task,
    deadline: Option<Instant>,
    /// How many runs the thread is inside, one within another where a
    /// Filesystem's own call starts another.
    depth: usize,
}

thread_local! {
    static TASK: RefCell<Current> = RefCell::default();
}

/// Run `run` as a run that stops waiting once `deadline` passes or its thread
/// is interrupted, as a Linux task stops a killable wait on a fatal signal. A
/// run within another keeps the outer one's interrupt.
pub fn run_task<T>(deadline: Option<Instant>, run: impl FnOnce() -> T) -> T {
    /// Puts back the deadline of the run around this one, however `run` ends,
    /// and lets an interrupt go once no run is left.
    struct Outer(Option<Instant>);

    impl Drop for Outer {
        fn drop(&mut self) {
            TASK.with(|task| {
                let mut running = task.borrow_mut();
                running.deadline = self.0;
                running.depth -= 1;
                if running.depth == 0 {
                    running.task.interrupted.store(false, Ordering::SeqCst);
                }
            });
        }
    }

    let _outer = Outer(TASK.with(|task| {
        let mut running = task.borrow_mut();
        running.depth += 1;
        std::mem::replace(&mut running.deadline, deadline)
    }));
    run()
}

/// Whether the calling thread's run is to stop waiting, as Linux's
/// `fatal_signal_pending(current)` answers.
pub(crate) fn fatal_signal_pending() -> bool {
    TASK.with(|task| {
        let running = task.borrow();
        running.task.interrupted.load(Ordering::SeqCst)
            || running.deadline.is_some_and(|at| Instant::now() >= at)
    })
}

/// The most a wait sleeps before it looks again whether its run is to stop.
const MAX_SLEEP: Duration = Duration::from_millis(10);

/// How long a wait sleeps before it looks again whether its run is to stop:
/// never past its deadline.
pub(crate) fn sleep_slice() -> Duration {
    TASK.with(|task| match task.borrow().deadline {
        Some(at) => at.saturating_duration_since(Instant::now()).min(MAX_SLEEP),
        None => MAX_SLEEP,
    })
}

/// A process has one Blocking, so a test counts only the waits of the thread
/// that asked to.
#[cfg(test)]
pub(crate) mod testing {
    use std::cell::RefCell;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

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

    /// What the thread `handle` runs answers, failing the test with `what` past
    /// `MAX_WAIT`, so a thread stuck on a lock fails rather than hangs.
    pub(crate) fn join<T>(what: &str, handle: std::thread::JoinHandle<T>) -> T {
        wait_until(what, || handle.is_finished());
        handle
            .join()
            .unwrap_or_else(|_| panic!("{what} must not panic"))
    }

    /// What the scoped thread `handle` runs answers, as `join` waits for one.
    pub(crate) fn join_scoped<T>(what: &str, handle: std::thread::ScopedJoinHandle<'_, T>) -> T {
        wait_until(what, || handle.is_finished());
        handle
            .join()
            .unwrap_or_else(|_| panic!("{what} must not panic"))
    }

    /// What `run` answers, each wait for a lock in it ending past `MAX_WAIT`, so a
    /// test thread waiting on a lock itself fails rather than hangs.
    pub(crate) fn within_max_wait<T>(run: impl FnOnce() -> T) -> T {
        super::run_task(Some(Instant::now() + MAX_WAIT), run)
    }

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
