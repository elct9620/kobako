//! Trap classification for the run path.
//!
//! Classifies a `wasmtime` run error into the engine-neutral `Trap`
//! kind (`Timeout` / `MemoryLimit` / `Other`) that each frontend maps
//! onto its own error surface, and hosts the epoch-deadline callback
//! that raises the wall-clock `TimeoutTrap`. The classification is a
//! pure function over the error's downcast chain so it can be exercised
//! from `cargo test` without any frontend.

use std::time::Instant;

use wasmtime::{StoreContextMut, UpdateDeadline};

use crate::invocation::Invocation;
use crate::limiter::MemoryLimitTrap;
use kobako_runtime::error::{SetupError, Trap};

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

/// Epoch delta that keeps the deadline effectively unreachable when no
/// wall-clock cap is configured. Half the epoch range rather than
/// `u64::MAX`: wasmtime adds the delta to the engine's current epoch,
/// which the process-wide ticker advances for the engine's whole
/// lifetime, so the full range overflows the sum (a panic under debug
/// overflow checks).
pub(crate) const NO_TIMEOUT_EPOCH_DELTA: u64 = u64::MAX / 2;

/// With no deadline the primed `NO_TIMEOUT_EPOCH_DELTA` keeps this from
/// firing; returning the same long extension keeps it inert as a defence
/// in depth.
pub(crate) fn epoch_deadline_callback(
    ctx: StoreContextMut<'_, Invocation>,
) -> wasmtime::Result<UpdateDeadline> {
    match ctx.data().deadline() {
        Some(deadline) if Instant::now() >= deadline => Err(wasmtime::Error::new(TimeoutTrap)),
        Some(_) => Ok(UpdateDeadline::Continue(1)),
        None => Ok(UpdateDeadline::Continue(NO_TIMEOUT_EPOCH_DELTA)),
    }
}

/// The message leaves out the ABI export symbol, so the Sandbox layer can
/// attach the caller's verb instead. A cap trap carries its own message,
/// since wasmtime's backtrace framing is noise there.
pub(crate) fn trap_from(err: wasmtime::Error) -> Trap {
    if let Some(t) = err.downcast_ref::<TimeoutTrap>() {
        Trap::Timeout(t.to_string())
    } else if let Some(t) = err.downcast_ref::<MemoryLimitTrap>() {
        Trap::MemoryLimit(t.to_string())
    } else {
        Trap::Other(other_trap_message(&err))
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

/// The template is built before any invocation, so neither cap can fire:
/// the memory cap is not yet armed and the probe Store's epoch deadline is
/// out of reach. Every failure here is a setup fault, not a trap.
pub(crate) fn instantiate_err(err: wasmtime::Error) -> SetupError {
    SetupError::Dead(format!("instantiate: {err}"))
}

#[cfg(test)]
mod tests {
    use super::{other_trap_message, trap_from, TimeoutTrap, NO_TIMEOUT_EPOCH_DELTA};
    use crate::invocation::Invocation;
    use crate::limiter::MemoryLimitTrap;
    use kobako_runtime::error::Trap;

    // The no-timeout priming delta is added to the engine's current
    // epoch inside wasmtime, and the process-wide ticker advances that
    // epoch from the first `shared_engine` call on — so the sum must
    // stay in range for a long-lived engine, not just a fresh one.
    // `increment_epoch` stands in for the ticker to make the ticked
    // state deterministic; under debug overflow checks an overflowing
    // delta panics right here.
    // @behavior S-119
    #[test]
    fn no_timeout_delta_survives_a_ticked_engine_epoch() {
        let engine = crate::cache::shared_engine().expect("shared engine must be constructible");
        engine.increment_epoch();
        let mut store = wasmtime::Store::new(engine, Invocation::new(None));
        store.set_epoch_deadline(NO_TIMEOUT_EPOCH_DELTA);
    }

    // @behavior OC-032
    #[test]
    fn trap_from_routes_timeout_trap_to_timeout() {
        let err = wasmtime::Error::new(TimeoutTrap);
        let expected = TimeoutTrap.to_string();
        assert!(matches!(trap_from(err), Trap::Timeout(msg) if msg == expected));
    }

    // @behavior OC-033
    #[test]
    fn trap_from_routes_memory_limit_trap_to_memory_limit() {
        let trap = MemoryLimitTrap::new(1 << 20, 1 << 19);
        let expected = trap.to_string();
        let err = wasmtime::Error::new(trap);
        assert!(matches!(trap_from(err), Trap::MemoryLimit(msg) if msg == expected));
    }

    // @behavior OC-028
    #[test]
    fn trap_from_falls_back_to_other_for_unknown_errors() {
        let err = wasmtime::Error::msg("some other wasmtime fault");
        assert!(matches!(trap_from(err), Trap::Other(_)));
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
            "a non-cap trap surfaced through Kobako::TrapError must carry the root trap reason, not only the backtrace framing; got: {msg}"
        );
        assert!(
            msg.contains("error while executing"),
            "a non-cap trap surfaced through Kobako::TrapError must keep the wasm backtrace framing; got: {msg}"
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
