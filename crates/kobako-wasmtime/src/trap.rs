//! Trap classification for the run path, and the setup fault a template's
//! instantiation answers.
//!
//! Classifies a `wasmtime` run error into the engine-neutral `Trap`
//! kind (`Timeout` / `MemoryLimit` / `Other`) that each frontend maps
//! onto its own error surface, and hosts the epoch-deadline callback
//! that raises the wall-clock `TimeoutTrap`. The classification is a
//! pure function over the error's downcast chain so it can be exercised
//! from `cargo test` without any frontend.

use std::time::{Duration, Instant};

use wasmtime::{Store, StoreHookState, UpdateDeadline};

use crate::invocation::Invocation;
use crate::limiter::MemoryLimiter;
use kobako_runtime::error::{SetupError, Trap};
use kobako_runtime::memcg::MemoryLimitExceeded;
use kobako_runtime::sched::{self, FatalSignal};
use kobako_runtime::snapshot::Completion;
use kobako_transport::envelope::Outcome;

/// Marker error returned from the epoch-deadline callback when the
/// wall-clock deadline is exceeded. Downcast from the wasmtime trap
/// error to classify the failure as a timeout `Trap`.
#[derive(Debug)]
pub(crate) struct TimeoutTrap;

impl std::fmt::Display for TimeoutTrap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "wall-clock deadline exceeded")
    }
}

impl std::error::Error for TimeoutTrap {}

/// Marker error a run ends with when its frontend interrupts it before the
/// deadline: a preview1 wait answers it EINTR, as a signal ends Linux's
/// `poll`; preview2, which has no EINTR, traps with it, and so does a guest
/// that only computes, at the next tick.
#[derive(Debug)]
pub(crate) struct Interrupted;

impl std::fmt::Display for Interrupted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "the guest was interrupted")
    }
}

impl std::error::Error for Interrupted {}

/// Epoch delta that keeps a test's Store, which installs no deadline
/// callback, from ever reaching its epoch deadline. Half the epoch range
/// rather than `u64::MAX`: wasmtime adds the delta to the engine's current
/// epoch, which the process-wide ticker advances for the engine's whole
/// lifetime, so the full range overflows the sum (a panic under debug
/// overflow checks).
#[cfg(test)]
pub(crate) const NO_TIMEOUT_EPOCH_DELTA: u64 = u64::MAX / 2;

/// Each tick, the guest's run takes its pending signal and stops on a fatal
/// one, as a Linux task does on its way back to user space: past the
/// deadline it traps as a timeout, interrupted it traps as `Interrupted`.
pub(crate) fn epoch_deadline_callback(
    ctx: StoreHookState<'_, Invocation>,
) -> wasmtime::Result<UpdateDeadline> {
    if ctx
        .data()
        .deadline()
        .is_some_and(|deadline| Instant::now() >= deadline)
    {
        return Err(wasmtime::Error::new(TimeoutTrap));
    }
    match fatal_signal_trap() {
        Some(trap) => Err(trap),
        None => Ok(UpdateDeadline::Continue(1)),
    }
}

/// The trap ending the calling thread's run once it takes its pending signal
/// and finds a fatal one, as the guest's code meets on each tick and a long
/// host call meets between its steps.
pub(crate) fn fatal_signal_trap() -> Option<wasmtime::Error> {
    sched::exit_to_user_mode().map(|signal| match signal {
        FatalSignal::Deadline => wasmtime::Error::new(TimeoutTrap),
        FatalSignal::Interrupt => wasmtime::Error::new(Interrupted),
    })
}

/// The message leaves out the ABI export symbol, so the Sandbox layer can
/// attach the caller's verb instead. A cap trap carries its own message,
/// since wasmtime's backtrace framing is noise there.
pub(crate) fn to_trap(err: wasmtime::Error) -> Trap {
    cap_trap_of(&err).unwrap_or_else(|| Trap::Other(other_trap_message(&err)))
}

/// Arm a Store's wall-clock deadline `limit` from now, or none. The callback
/// runs every tick either way, as an unbounded run still stops on a fatal
/// signal.
pub(crate) fn arm_deadline(store: &mut Store<Invocation>, limit: Option<Duration>) {
    store.epoch_deadline_callback(epoch_deadline_callback);
    store
        .data_mut()
        .set_deadline(limit.map(|limit| Instant::now() + limit));
    store.set_epoch_deadline(1);
}

/// The class a guest's interpreter raises once its allocator answers no
/// room, as mruby's and CRuby's do.
const OUT_OF_MEMORY: &str = "NoMemoryError";

/// `trap` ending a run, as `limiter` attributes it: once the budget refused
/// a growth, a run that fails for another reason than its deadline failed
/// for want of the room, as an allocator with none traps or a GC heap
/// allocation fails.
pub(crate) fn charged_to_budget(trap: Trap, limiter: &MemoryLimiter) -> Trap {
    match (limiter.refused(), trap) {
        (Some(refused), Trap::Other(_)) => Trap::MemoryLimit(refused.to_string()),
        (_, trap) => trap,
    }
}

/// What `outcome` completes a run as: a guest that ended raising
/// `NoMemoryError` after the budget refused a growth failed as the budget's,
/// while one that collected and went on answers its own outcome.
pub(crate) fn completion_of(outcome: Outcome, limiter: &MemoryLimiter) -> Completion {
    match (&outcome, limiter.refused()) {
        (Outcome::Panic(panic), Some(refused)) if panic.error.name == OUT_OF_MEMORY => {
            Completion::Trap(Trap::MemoryLimit(refused.to_string()))
        }
        _ => Completion::Outcome(outcome),
    }
}

/// The cap trap `err` carries, if a cap fired: a setup step that fails
/// for any other reason words its own message instead.
pub(crate) fn cap_trap_of(err: &wasmtime::Error) -> Option<Trap> {
    if let Some(t) = err.downcast_ref::<TimeoutTrap>() {
        Some(Trap::Timeout(t.to_string()))
    } else {
        err.downcast_ref::<MemoryLimitExceeded>()
            .map(|t| Trap::MemoryLimit(t.to_string()))
    }
}

/// wasmtime's `Display` shows only the backtrace framing; the real trap
/// reason is the chain's root cause and would otherwise be dropped, leaving
/// a guest fault undiagnosable.
fn other_trap_message(err: &wasmtime::Error) -> String {
    let display = format!("{}", err);
    let root = err.root_cause().to_string();
    if display.contains(&root) {
        display
    } else {
        format!("{display}\n\n{root}")
    }
}

/// The template is built before any invocation, so no invocation cap can
/// fire: the memory budget is not yet armed, and the probe's own setup
/// deadline is told apart where it fires. Every failure here is a setup
/// fault, not a trap.
pub(crate) fn to_setup_error(err: wasmtime::Error) -> SetupError {
    SetupError::Dead(format!("instantiate: {err}"))
}

#[cfg(test)]
mod tests {
    use super::{other_trap_message, to_trap, TimeoutTrap, NO_TIMEOUT_EPOCH_DELTA};
    use kobako_runtime::error::Trap;
    use kobako_runtime::memcg::MemoryLimitExceeded;

    // The no-timeout priming delta is added to the engine's current
    // epoch inside wasmtime, and the process-wide ticker advances that
    // epoch from the first `load_engine` call on — so the sum must
    // stay in range for a long-lived engine, not just a fresh one.
    // `increment_epoch` stands in for the ticker to make the ticked
    // state deterministic; under debug overflow checks an overflowing
    // delta panics right here.
    // @behavior S-119
    #[test]
    fn no_timeout_delta_survives_a_ticked_engine_epoch() {
        let engine = crate::cache::load_engine().expect("shared engine must be constructible");
        engine.increment_epoch();
        let mut store = wasmtime::Store::new(engine, ());
        store.set_epoch_deadline(NO_TIMEOUT_EPOCH_DELTA);
    }

    // @behavior OC-032
    #[test]
    fn to_trap_routes_timeout_trap_to_timeout() {
        let err = wasmtime::Error::new(TimeoutTrap);
        let expected = TimeoutTrap.to_string();
        assert!(matches!(to_trap(err), Trap::Timeout(msg) if msg == expected));
    }

    // @behavior OC-033
    #[test]
    fn to_trap_routes_memory_limit_exceeded_to_memory_limit() {
        let trap = MemoryLimitExceeded::new(1 << 20, 1 << 19);
        let expected = trap.to_string();
        let err = wasmtime::Error::new(trap);
        assert!(matches!(to_trap(err), Trap::MemoryLimit(msg) if msg == expected));
    }

    // @behavior OC-028
    #[test]
    fn to_trap_falls_back_to_other_for_unknown_errors() {
        let err = wasmtime::Error::msg("some other wasmtime fault");
        assert!(matches!(to_trap(err), Trap::Other(_)));
    }

    // A guest hard trap reaches the host as a wasmtime error whose Display is
    // only the backtrace framing, with the trap reason buried as the chain's
    // root cause. The named-capture regex bug surfaced as exactly this shape.
    // @behavior OC-029
    #[test]
    fn other_trap_message_surfaces_buried_trap_reason() {
        let err = wasmtime::Error::msg("wasm trap: indirect call type mismatch")
            .context("error while executing at wasm backtrace:\n  0: 0x1 - <unknown>");
        let msg = other_trap_message(&err);
        assert!(
            msg.contains("indirect call type mismatch"),
            "a non-cap trap surfaced to the host must carry the root trap reason, not only the backtrace framing; got: {msg}"
        );
        assert!(
            msg.contains("error while executing"),
            "a non-cap trap surfaced to the host must keep the wasm backtrace framing; got: {msg}"
        );
    }

    // A flat error (no cause chain) is its own root_cause; appending it would
    // duplicate the whole message.
    // @behavior OC-030
    #[test]
    fn other_trap_message_does_not_duplicate_a_flat_error() {
        let err = wasmtime::Error::msg("plain fault");
        assert_eq!(other_trap_message(&err), "plain fault");
    }
}
