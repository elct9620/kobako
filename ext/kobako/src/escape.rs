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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use kobako_runtime::fork::ProcessLocal;
use kobako_runtime::sched::Current;
use magnus::value::BoxValue;
use magnus::{Error as MagnusError, Ruby, Value};

use super::handoff::{self, Line};

/// The runs in progress, each under the Fiber that started it. Only a thread
/// holding the GVL reads or writes it, so a fork, which holds the GVL too, never
/// finds it held.
static RUNS: ProcessLocal<HashMap<FiberId, Arc<RunState>>> = ProcessLocal::new();

/// One outermost run on a Fiber and the runs nested in its callbacks.
pub(super) struct RunState {
    /// The task every thread carrying the run out shares, so a lock its
    /// guest holds knows a nested run's guest as the same holder.
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
}

impl RunState {
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
    /// Fiber is in, an outermost one starts a run of its own.
    pub(super) fn enter() -> Self {
        let fiber = fiber_id();
        let state = RUNS.with(|runs| {
            let runs = runs.get_or_insert_with(HashMap::new);
            runs.entry(fiber)
                .or_insert_with(|| {
                    Arc::new(RunState {
                        task: Current::fresh(),
                        held: Mutex::new(None),
                        pending: Arc::default(),
                        depth: Mutex::new(0),
                        lines: Mutex::new(Vec::new()),
                        pins: Mutex::new(Vec::new()),
                    })
                })
                .clone()
        });
        *lock(&state.depth) += 1;
        Self {
            fiber,
            state,
            finished: false,
        }
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
            RUNS.with(|runs| runs.as_mut().map(|runs| runs.remove(&self.fiber)));
        }
    }
}

/// Forget the run `fiber` is in, as the collector freed the Fiber while it
/// was suspended in a callback.
pub(super) fn forget(fiber: FiberId) {
    RUNS.with(|runs| runs.as_mut().map(|runs| runs.remove(&fiber)));
}

/// A Fiber as a run knows it: the Fiber itself, which the run's `Run` keeps on
/// the Fiber's own stack, so the collector, scanning that stack, never moves
/// it while the run lasts.
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
    RUNS.with(|runs| runs.as_ref().and_then(|runs| runs.get(&fiber).cloned()))
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
