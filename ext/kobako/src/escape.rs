//! What a Host App's callback raises that is not a `StandardError`:
//! `Interrupt`, `exit`, `throw`, `Timeout`, and any interrupt delivered to the
//! Thread while a run gives up the GVL. These belong to the host, so one is held
//! here instead of answering the guest with it, the invocation ends at its next
//! host call, and the run raises it again once control is back in Ruby.
//!
//! The slot is the run's, kept for the Fiber that started it, since a run's
//! callbacks come back on that Fiber whatever native thread it is on, and
//! another Fiber's run on the same Thread is a run of its own. A nested
//! invocation takes what its own run held before the outer callback sees it,
//! so the outer one holds it next. What a run held belongs to it alone: one
//! ending by a panic drops it, and a Fiber outside any run holds nothing,
//! since no run would raise it again.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use kobako_runtime::fork::ProcessLocal;
use kobako_runtime::sched::Current;
use kobako_runtime::wait::{do_poll, poll_wait, PollTable, WaitQueueHead};
use magnus::value::BoxValue;
use magnus::{Error as MagnusError, Ruby, Value};

use super::handoff::{self, Line};

/// The runs in progress, each under the Fiber that started it, and the forks
/// holding new ones back. A forked child starts from none, as the process
/// made none of its own.
static RUNS: ProcessLocal<Runs> = ProcessLocal::new();

#[derive(Default)]
struct Runs {
    by_fiber: HashMap<FiberId, Arc<RunState>>,
    /// The guests of runs whose caller was collected, in progress until each
    /// ends. The Fiber's address is free for another run once collected.
    ending: Vec<Arc<Line>>,
    /// The forks under way, each found no run in progress.
    forks: usize,
    /// The outermost runs waiting for the forks under way to return.
    waiting: Vec<(FiberId, Arc<RunState>)>,
    /// Woken as the last fork under way returns.
    opened: Arc<WaitQueueHead>,
}

impl Runs {
    /// Whether a run is in progress, one whose caller was collected included
    /// until its guest ends.
    fn in_progress(&mut self) -> bool {
        self.ending.retain(|line| !line.has_ended());
        !self.by_fiber.is_empty() || !self.ending.is_empty()
    }
}

/// Run `f` on the runs in progress.
fn with_runs<R>(f: impl FnOnce(&mut Runs) -> R) -> R {
    RUNS.with(|runs| f(runs.get_or_insert_with(Runs::default)))
}

/// Forget the run on `fiber`.
fn remove(fiber: FiberId) {
    with_runs(|runs| runs.by_fiber.remove(&fiber));
}

/// Wait, without the GVL, until the last fork under way enters the run on
/// `fiber`. An interrupt ends the wait, and the run gives back its place,
/// whether still waiting or entered as the interrupt came.
fn wait_for_forks(ruby: &Ruby, fiber: FiberId) -> Result<(), MagnusError> {
    let entered = |table: Option<&mut PollTable>| {
        with_runs(|runs| {
            poll_wait(&runs.opened, table);
            runs.by_fiber.contains_key(&fiber).then_some(())
        })
    };
    loop {
        if let Ok(Some(())) = do_poll(|| None, &entered) {
            return Ok(());
        }
        if let Err(interrupt) = ruby.thread_check_ints() {
            with_runs(|runs| {
                runs.waiting.retain(|(waiting, _)| *waiting != fiber);
                runs.by_fiber.remove(&fiber);
            });
            return Err(interrupt);
        }
    }
}

/// Hold new runs back for a fork, unless a run is in progress, which a forked
/// child would inherit the locks of. Checking and holding back are one step,
/// so no run starts between them.
pub(super) fn hold_runs_back() -> bool {
    with_runs(|runs| {
        let held = !runs.in_progress();
        if held {
            runs.forks += 1;
        }
        held
    })
}

/// Let runs in again as a fork that held them back returns. The last one
/// enters every waiting run in the same step, so no fork closes the gate
/// before they do. A forked child held none back.
pub(super) fn let_runs_in() {
    let opened = with_runs(|runs| {
        runs.forks = runs.forks.saturating_sub(1);
        if runs.forks == 0 {
            let waiting = std::mem::take(&mut runs.waiting);
            runs.by_fiber.extend(waiting);
        }
        runs.opened.clone()
    });
    opened.wake_up();
}

/// One outermost run on a Fiber and the runs nested in its callbacks.
pub(super) struct RunState {
    /// The task every thread carrying the run out shares, so a lock its
    /// guest holds knows a nested run's guest, or a guest of a run another
    /// Fiber of the Thread starts meanwhile, as the same owner.
    task: Current,
    held: Mutex<Option<Escape>>,
    /// Whether `held` holds an error, read by the guest's own thread.
    pending: Arc<AtomicBool>,
    depth: Mutex<usize>,
    /// The line to each guest of the run and its nested runs, innermost last.
    lines: Mutex<Vec<Arc<Line>>>,
    /// The Ruby objects the run's guests reach, one entry for each pin. The
    /// run's caller marks them, so the collector keeps them where they stand
    /// while the caller does, and frees them with a caller that never came back.
    pins: Mutex<Vec<rb_sys::VALUE>>,
    /// The Ruby Thread the run started on, pinned so it stays where it stands.
    thread: rb_sys::VALUE,
}

impl RunState {
    fn is_running(&self) -> bool {
        *lock(&self.depth) > 0
    }

    /// Keep `value` for the run.
    pub(super) fn pin(&self, value: rb_sys::VALUE) {
        lock(&self.pins).push(value);
    }

    /// Let go of one pin of `value`.
    pub(super) fn unpin(&self, value: rb_sys::VALUE) {
        let mut pins = lock(&self.pins);
        if let Some(at) = pins.iter().position(|&pinned| pinned == value) {
            pins.swap_remove(at);
        }
    }

    /// The Ruby objects the run keeps, for its caller to mark.
    pub(super) fn pinned(&self) -> Vec<rb_sys::VALUE> {
        lock(&self.pins).clone()
    }

    pub(super) fn task(&self) -> &Current {
        &self.task
    }

    /// Run `serve` with `line` as the run's innermost, so a guest block its
    /// callbacks call finds the guest it belongs to.
    pub(super) fn lending<R>(&self, line: &Arc<Line>, serve: impl FnOnce() -> R) -> R {
        /// Takes the line back however `serve` ends.
        struct Lent<'a>(&'a RunState);

        impl Drop for Lent<'_> {
            fn drop(&mut self) {
                lock(&self.0.lines).pop();
            }
        }

        lock(&self.lines).push(line.clone());
        let _lent = Lent(self);
        serve()
    }

    /// The line to the run's innermost guest.
    pub(super) fn innermost_line(&self) -> Option<Arc<Line>> {
        lock(&self.lines).last().cloned()
    }

    /// Whether the run holds an error, as its guest's thread asks it.
    pub(super) fn pending_flag(&self) -> Arc<AtomicBool> {
        self.pending.clone()
    }
}

/// One invocation or compile on this Fiber, which raises what it held once
/// it is back in Ruby.
pub(super) struct Run {
    fiber: FiberId,
    state: Arc<RunState>,
    finished: bool,
}

impl Run {
    /// Enter a run on the calling Fiber: a nested one shares the run its
    /// Fiber is in, an outermost one starts a run of its own. Locks know an
    /// outermost one as the run another Fiber of the Thread has in progress,
    /// as CRuby refuses a Mutex another Fiber of the Thread owns, unless
    /// `scheduler_waits` says a Fiber scheduler takes the run's waits, as
    /// CRuby leaves a Mutex wait to the scheduler.
    ///
    /// An outermost run waits while a fork is under way, counted in
    /// `waiting`, its Sandbox's runs waiting, and raises what interrupts that
    /// wait.
    pub(super) fn enter(
        ruby: &Ruby,
        waiting: &AtomicUsize,
        scheduler_waits: bool,
    ) -> Result<Self, MagnusError> {
        let fiber = fiber_id();
        let state = match with_runs(|runs| runs.by_fiber.get(&fiber).cloned()) {
            Some(state) => state,
            None => {
                let state = start(scheduler_waits);
                let entered = with_runs(|runs| {
                    if runs.forks == 0 {
                        runs.by_fiber.insert(fiber, state.clone());
                    } else {
                        runs.waiting.push((fiber, state.clone()));
                    }
                    runs.forks == 0
                });
                if !entered {
                    handoff::count_waiting(waiting, || wait_for_forks(ruby, fiber))?;
                }
                state
            }
        };
        *lock(&state.depth) += 1;
        Ok(Self {
            fiber,
            state,
            finished: false,
        })
    }

    pub(super) fn state(&self) -> &Arc<RunState> {
        &self.state
    }

    /// Raise what this run held.
    pub(super) fn finish(mut self) -> Result<(), MagnusError> {
        self.finished = true;
        match take(&self.state) {
            Some(held) => Err(held.error),
            None => Ok(()),
        }
    }
}

impl Drop for Run {
    fn drop(&mut self) {
        if !self.finished {
            take(&self.state);
        }
        let mut depth = lock(&self.state.depth);
        *depth -= 1;
        if *depth == 0 {
            drop(depth);
            remove(self.fiber);
        }
    }
}

/// Start an outermost run on the calling Fiber. Unless a Fiber scheduler takes
/// its waits, locks know it as a run another Fiber of the Thread has in
/// progress: no such run goes on while this Thread waits.
fn start(scheduler_waits: bool) -> Arc<RunState> {
    // SAFETY: `rb_thread_current` reads the calling thread's own execution
    // context, which a thread holding the GVL may.
    let thread = unsafe { rb_sys::rb_thread_current() };
    let task = (!scheduler_waits)
        .then(|| running_on(thread))
        .flatten()
        .map_or_else(Current::default, |running| {
            Current::with_owner(&running.task)
        });
    Arc::new(RunState {
        task,
        held: Mutex::new(None),
        pending: Arc::default(),
        depth: Mutex::new(0),
        lines: Mutex::new(Vec::new()),
        pins: Mutex::new(vec![thread]),
        thread,
    })
}

/// A run another Fiber has in progress on `thread`.
fn running_on(thread: rb_sys::VALUE) -> Option<Arc<RunState>> {
    let on_thread: Vec<Arc<RunState>> = with_runs(|runs| {
        runs.by_fiber
            .values()
            .filter(|state| state.thread == thread)
            .cloned()
            .collect()
    });
    on_thread.into_iter().find(|state| state.is_running())
}

/// Move the run `fiber` is in to the guest `line` leads to, as the collector
/// freed the Fiber while it was suspended in a callback: the run stays in
/// progress until that guest ends, and the Fiber's address is free for
/// another run.
pub(super) fn forget(fiber: FiberId, line: &Arc<Line>) {
    with_runs(|runs| {
        runs.by_fiber.remove(&fiber);
        if !line.has_ended() {
            runs.ending.push(line.clone());
        }
    });
}

/// A Fiber as a run knows it: the Fiber itself, which the run's `RunCaller`
/// keeps on the Fiber's own stack, so the collector, scanning that stack,
/// never moves it while the run lasts.
pub(crate) type FiberId = rb_sys::VALUE;

/// The calling Fiber, asked while holding the GVL.
pub(crate) fn fiber_id() -> FiberId {
    // SAFETY: `rb_fiber_current` reads the calling thread's own execution
    // context, which a thread holding the GVL may.
    unsafe { rb_sys::rb_fiber_current() }
}

/// The run the calling Fiber is in, asked while holding the GVL.
pub(super) fn current_run() -> Option<Arc<RunState>> {
    let fiber = fiber_id();
    with_runs(|runs| runs.by_fiber.get(&fiber).cloned())
}

/// Whether the calling Fiber is inside a run, which holds what reaches it.
pub(super) fn is_in_run() -> bool {
    current_run().is_some()
}

fn take(state: &RunState) -> Option<Escape> {
    let held = lock(&state.held).take();
    state.pending.store(false, Ordering::SeqCst);
    held
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The error and, when it carries one, its exception kept rooted while
/// the guest finishes unwinding.
struct Escape {
    error: MagnusError,
    _rooted: Option<BoxValue<Value>>,
}

// SAFETY: an `Escape` is made, read, and dropped only on a thread holding the
// GVL, which is what its Ruby values need; the run's other threads see only the
// `pending` flag beside it.
unsafe impl Send for Escape {}

/// Hold `error` unless it is a `StandardError`, which comes back for the
/// callback to answer the guest with.
pub(super) fn hold(ruby: &Ruby, error: MagnusError) -> Result<(), MagnusError> {
    if error.is_kind_of(ruby.exception_standard_error()) {
        return Err(error);
    }
    keep(error);
    Ok(())
}

/// Run what is pending on this Thread, holding what it raises, whatever its
/// class: no callback asked for it, so no guest is answered with it.
pub(super) fn hold_interrupts() {
    let ruby = Ruby::get().expect("a Thread that was not given up holds the GVL");
    if let Err(error) = ruby.thread_check_ints() {
        keep(error);
    }
}

/// Keep `error` for the host; the first one kept wins. Outside any run
/// nothing would raise it again, so it goes.
pub(super) fn keep(error: MagnusError) {
    let Some(state) = current_run() else {
        return;
    };
    lock(&state.held).get_or_insert_with(|| Escape {
        _rooted: error.value().map(BoxValue::new),
        error,
    });
    state.pending.store(true, Ordering::SeqCst);
}

/// Whether the run holds an error for the host, asked on the run's caller or
/// on its guest's thread.
pub(super) fn pending() -> bool {
    match handoff::pending_flag() {
        Some(pending) => pending.load(Ordering::SeqCst),
        None => current_run().is_some_and(|state| state.pending.load(Ordering::SeqCst)),
    }
}
