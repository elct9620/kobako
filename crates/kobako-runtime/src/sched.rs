//! Who is running and how it waits, beneath every lock, as Linux's
//! `kernel/sched` is: a lock asks only for its wait to happen, so nothing
//! above it knows what waiting means to the frontend. A run past its deadline,
//! or one its frontend interrupted, stops waiting, as a fatal signal ends
//! Linux's killable waits, so one Sandbox holding a lock never holds another's
//! thread past that one's own end.

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use crate::wait::Waiter;

/// How a thread waits for a lock another thread holds, or for what another
/// thread brings. A frontend whose threads share a lock of their own, as
/// Ruby's threads share the GVL, gives that lock up for the wait, or a holder
/// that needs it to finish never could.
pub trait Blocking: Send + Sync {
    /// Run `wait`, which returns once the lock is taken or the event arrived.
    fn wait(&self, wait: &mut dyn FnMut());

    /// Take the signal a waiting thread was woken for, interrupting its task
    /// when the signal is to end the run, as Linux handles a pending signal on
    /// the way back to user space. A wait the signal leaves alone goes on, as
    /// Linux restarts a call a handled signal woke.
    fn handle_signal(&self) {}

    /// The task the calling thread carries out, where the frontend tracks it
    /// apart from the thread, as a Ruby Fiber's run is its own whatever
    /// thread the Fiber runs on. `None` leaves the thread its own task.
    fn current(&self) -> Option<Current> {
        None
    }
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

/// Who is running, as a lock records Linux's `current` as its owner.
pub(crate) fn current() -> TaskId {
    Current::get().state().id
}

/// A task as a lock knows its holder, the same on every thread carrying the
/// task out, as Linux records a task and never the CPU it ran on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TaskId(u64);

impl TaskId {
    fn fresh() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

/// The task a thread carries out, as Linux's `current`, and when its run is
/// to stop. Every thread carrying it shares one, so a run handed from thread to
/// thread stays one task.
#[derive(Clone)]
pub struct Current(Arc<Mutex<State>>);

struct State {
    id: TaskId,
    task: Task,
    deadline: Option<Instant>,
    /// How many interruptible stretches the task is inside: a frontend's
    /// around its run, and a run within another where a Filesystem's own call
    /// starts another.
    depth: usize,
}

thread_local! {
    // What the thread carries out in another's place, while it does.
    static CARRIED: RefCell<Option<Current>> = const { RefCell::new(None) };
    // The thread's own task, carried whenever it carries out no other.
    static OWN: Current = Current::fresh();
}

impl Current {
    /// A task nobody carries out yet.
    pub fn fresh() -> Self {
        Self(Arc::new(Mutex::new(State {
            id: TaskId::fresh(),
            task: Task::default(),
            deadline: None,
            depth: 0,
        })))
    }

    /// What the calling thread carries out: the task it carries in another's
    /// place, the one its frontend tracks, or its own.
    pub fn get() -> Self {
        CARRIED
            .with(|carried| carried.borrow().clone())
            .or_else(|| BLOCKING.get().and_then(|blocking| blocking.current()))
            .unwrap_or_else(|| OWN.with(Clone::clone))
    }

    /// Carry this task out on the calling thread while `f` runs, as a thread
    /// taking a run over from another, putting back what it carried before.
    pub fn carry<T>(&self, f: impl FnOnce() -> T) -> T {
        /// Puts back what the thread carried, however `f` ends.
        struct Before(Option<Current>);

        impl Drop for Before {
            fn drop(&mut self) {
                CARRIED.with(|carried| *carried.borrow_mut() = self.0.take());
            }
        }

        let _before = Before(CARRIED.with(|carried| carried.replace(Some(self.clone()))));
        f()
    }

    /// What a frontend interrupts or signals this task through.
    pub fn task(&self) -> Task {
        self.state().task.clone()
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// A thread as its frontend may interrupt it from another thread, as a signal
/// reaches a Linux task.
#[derive(Clone, Default)]
pub struct Task {
    interrupted: Arc<AtomicBool>,
    signalled: Arc<AtomicBool>,
    /// The waiter the thread sleeps on, woken by an interrupt as
    /// `signal_wake_up` wakes a sleeping task.
    sleeping: Arc<Mutex<Option<Arc<Waiter>>>>,
}

impl Task {
    /// The calling thread's.
    pub fn current() -> Self {
        Current::get().state().task.clone()
    }

    /// Have every wait of the run this thread carries out stop, as a fatal
    /// signal ends a killable wait; it holds until that run ends.
    pub fn interrupt(&self) {
        self.interrupted.store(true, Ordering::SeqCst);
        self.wake_up();
    }

    /// Have the wait the thread's run sleeps in hand a signal that may not be
    /// fatal to the registered `Blocking`, as `signal_wake_up` wakes a task
    /// for its pending signal; it holds until handled or that run ends.
    pub fn signal(&self) {
        self.signalled.store(true, Ordering::SeqCst);
        self.wake_up();
    }

    fn wake_up(&self) {
        if let Some(waiter) = self.sleeping().as_ref() {
            waiter.wake();
        }
    }

    fn sleeping(&self) -> std::sync::MutexGuard<'_, Option<Arc<Waiter>>> {
        self.sleeping.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Have an interrupt of the calling thread wake `waiter` while `sleep` runs,
/// as a task sleeping in `TASK_KILLABLE` is woken by a fatal signal.
pub(crate) fn sleep_on<T>(waiter: &Arc<Waiter>, sleep: impl FnOnce() -> T) -> T {
    /// Takes the waiter back however `sleep` ends.
    struct Awake(Task);

    impl Drop for Awake {
        fn drop(&mut self) {
            *self.0.sleeping() = None;
        }
    }

    let task = Task::current();
    *task.sleeping() = Some(waiter.clone());
    let _awake = Awake(task);
    sleep()
}

/// Run `run` as a run that stops waiting once `deadline` passes or its thread
/// is interrupted, as a Linux task stops a killable wait on a fatal signal. A
/// run within another keeps the outer one's interrupt.
pub fn run_task<T>(deadline: Option<Instant>, run: impl FnOnce() -> T) -> T {
    /// Puts back the deadline of the run around this one, however `run` ends.
    struct Outer(Current, Option<Instant>);

    impl Drop for Outer {
        fn drop(&mut self) {
            self.0.state().deadline = self.1;
        }
    }

    run_interruptible(|| {
        let current = Current::get();
        let outer = std::mem::replace(&mut current.state().deadline, deadline);
        let _outer = Outer(current, outer);
        run()
    })
}

/// Run `run` as a stretch its frontend may interrupt, as a frontend brackets
/// all it does for one call around the run itself. An interrupt landing
/// anywhere in it lasts until the outermost stretch ends, so it stops the
/// waits of the run it was meant for and never those of the thread's next.
pub fn run_interruptible<T>(run: impl FnOnce() -> T) -> T {
    /// Lets an interrupt go once no stretch is left, however `run` ends.
    struct Stretch(Current);

    impl Drop for Stretch {
        fn drop(&mut self) {
            let mut running = self.0.state();
            running.depth -= 1;
            if running.depth == 0 {
                running.task.interrupted.store(false, Ordering::SeqCst);
                running.task.signalled.store(false, Ordering::SeqCst);
            }
        }
    }

    let current = Current::get();
    current.state().depth += 1;
    let _stretch = Stretch(current);
    run()
}

/// Why a run is to stop, as the fatal signal pending on a Linux task says
/// what ended it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FatalSignal {
    /// The run passed its deadline.
    Deadline,
    /// The run's frontend interrupted it.
    Interrupt,
}

/// The fatal signal pending on the calling thread's run, its deadline read
/// first; `None` while the run may go on waiting.
pub fn pending_fatal_signal() -> Option<FatalSignal> {
    let current = Current::get();
    let running = current.state();
    if running.deadline.is_some_and(|at| Instant::now() >= at) {
        Some(FatalSignal::Deadline)
    } else if running.task.interrupted.load(Ordering::SeqCst) {
        Some(FatalSignal::Interrupt)
    } else {
        None
    }
}

/// Whether the calling thread's run is to stop waiting, as Linux's
/// `fatal_signal_pending(current)` answers.
pub(crate) fn fatal_signal_pending() -> bool {
    pending_fatal_signal().is_some()
}

/// Whether a signal waits for the calling thread to handle it, as Linux's
/// `signal_pending(current)` answers.
pub(crate) fn signal_pending() -> bool {
    Task::current().signalled.load(Ordering::SeqCst)
}

/// Hand the signal pending on the calling thread to the registered
/// `Blocking`, as a woken wait returns to have it handled; the wait then looks
/// at `fatal_signal_pending` again. It must hold no lock that never waits
/// through the `Blocking`, as the frontend may take its own to handle it.
pub(crate) fn handle_signal() {
    let signalled = Task::current().signalled.swap(false, Ordering::SeqCst);
    if let (true, Some(blocking)) = (signalled, BLOCKING.get()) {
        blocking.handle_signal();
    }
}

/// When the calling thread's run is to stop, once it has a deadline.
pub(crate) fn deadline() -> Option<Instant> {
    Current::get().state().deadline
}

/// The most a wait sleeps before it looks again whether its run is to stop.
const MAX_SLEEP: Duration = Duration::from_millis(10);

/// How long a wait sleeps before it looks again whether its run is to stop:
/// never past its deadline.
pub(crate) fn sleep_slice() -> Duration {
    match deadline() {
        Some(at) => at.saturating_duration_since(Instant::now()).min(MAX_SLEEP),
        None => MAX_SLEEP,
    }
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
        static SIGNALS: RefCell<Option<Arc<AtomicUsize>>> = const { RefCell::new(None) };
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

        fn handle_signal(&self) {
            SIGNALS.with(|signals| {
                if let Some(signals) = signals.borrow().as_ref() {
                    signals.fetch_add(1, Ordering::SeqCst);
                }
            });
        }
    }

    /// Counts each wait this thread makes through the registered Blocking.
    pub(crate) fn count_waits() -> Arc<AtomicUsize> {
        BLOCKING.get_or_init(|| Box::new(CountsWaits));
        let counts = Arc::<AtomicUsize>::default();
        COUNTS.with(|held| *held.borrow_mut() = Some(counts.clone()));
        counts
    }

    /// Counts each signal the registered Blocking handles for this thread,
    /// leaving every wait to go on.
    pub(crate) fn count_signals() -> Arc<AtomicUsize> {
        BLOCKING.get_or_init(|| Box::new(CountsWaits));
        let signals = Arc::<AtomicUsize>::default();
        SIGNALS.with(|held| *held.borrow_mut() = Some(signals.clone()));
        signals
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // @behavior RT-403
    #[test]
    fn an_interrupt_around_a_run_stops_that_run_alone() {
        let (during_setup, after_teardown) = run_interruptible(|| {
            Task::current().interrupt();
            let during_setup = run_task(None, pending_fatal_signal);
            Task::current().interrupt();
            (during_setup, pending_fatal_signal())
        });
        let next = run_task(None, pending_fatal_signal);
        assert_eq!(
            (during_setup, after_teardown, next),
            (
                Some(FatalSignal::Interrupt),
                Some(FatalSignal::Interrupt),
                None
            ),
            "an interrupt while a run is set up must stop that run's waits, and \
             one while it is torn down must not stop the thread's next run"
        );
    }
}
