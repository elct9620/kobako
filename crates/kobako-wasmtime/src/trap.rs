//! Trap classification for the run path.
//!
//! Classifies a `wasmtime` run error into the engine-neutral `Trap`
//! kind (`Timeout` / `MemoryLimit` / `Other`) that each frontend maps
//! onto its own error surface, and hosts the epoch-deadline callback
//! that raises the wall-clock `TimeoutTrap`. The classification is a
//! pure function over the error's downcast chain so it can be exercised
//! from `cargo test` without any frontend; the trap marker types
//! themselves live in `crate::invocation` (where the limiter / callback
//! construct them).

use std::time::Instant;

use wasmtime::{StoreContextMut, UpdateDeadline};

use crate::invocation::{Invocation, MemoryLimitTrap, TimeoutTrap};
use kobako_runtime::error::{SetupError, Trap};

/// Epoch delta that keeps the deadline effectively unreachable when no
/// wall-clock cap is configured. Half the epoch range rather than
/// `u64::MAX`: wasmtime adds the delta to the engine's current epoch,
/// which the process-wide ticker advances for the engine's whole
/// lifetime, so the full range overflows the sum (a panic under debug
/// overflow checks).
pub(crate) const NO_TIMEOUT_EPOCH_DELTA: u64 = u64::MAX / 2;

/// Epoch-deadline callback installed on every Store. Read the per-run
/// wall-clock deadline from `Invocation` and trap with
/// `TimeoutTrap` once the deadline has passed; otherwise extend the
/// next check by one tick of the process-wide epoch ticker. When the
/// deadline is `None` the callback should not fire under the normal
/// `Driver` invoke flow because
/// `NO_TIMEOUT_EPOCH_DELTA` is primed; returning the same long
/// extension keeps the callback inert as a defence in depth.
pub(crate) fn epoch_deadline_callback(
    ctx: StoreContextMut<'_, Invocation>,
) -> wasmtime::Result<UpdateDeadline> {
    match ctx.data().deadline() {
        Some(deadline) if Instant::now() >= deadline => Err(wasmtime::Error::new(TimeoutTrap)),
        Some(_) => Ok(UpdateDeadline::Continue(1)),
        None => Ok(UpdateDeadline::Continue(NO_TIMEOUT_EPOCH_DELTA)),
    }
}

/// Classify a wasmtime call error into a neutral `Trap`. Pure function
/// over the error's downcast chain, so the kind routing is exercisable
/// from `cargo test` without any frontend. The ABI export symbol
/// (`__kobako_eval` / `__kobako_run`) is deliberately omitted from the
/// message — the Sandbox layer attaches the user-facing verb
/// (`Sandbox#eval` / `Sandbox#run`) so the message reads in caller
/// vocabulary rather than ABI vocabulary.
///
/// For the configured-cap paths the trap's own `std::fmt::Display`
/// carries the user-facing reason (`"wall-clock deadline exceeded"`,
/// `"linear memory growth exceeded memory_limit: ..."`); the wasmtime
/// outer wrapper would otherwise surface only the `"error while
/// executing at wasm backtrace: ..."` framing, which is operator noise
/// on a cap trap. For any other error the framing is kept but the
/// chain's root cause is appended (see `other_trap_message`) so the
/// real trap reason survives.
pub(crate) fn trap_from(err: wasmtime::Error) -> Trap {
    if let Some(t) = err.downcast_ref::<TimeoutTrap>() {
        Trap::Timeout(t.to_string())
    } else if let Some(t) = err.downcast_ref::<MemoryLimitTrap>() {
        Trap::MemoryLimit(t.to_string())
    } else {
        Trap::Other(other_trap_message(&err))
    }
}

/// Compose the message for a non-cap trap. wasmtime's `Display` surfaces only
/// the `"error while executing at wasm backtrace: ..."` framing; the actual
/// trap reason (e.g. `"wasm trap: indirect call type mismatch"`) is the
/// chain's root cause and would otherwise be dropped, making real guest
/// faults undiagnosable. Append the root cause unless the framing already
/// carries it. Pure so it can be exercised from `cargo test` without the
/// magnus surface.
fn other_trap_message(err: &wasmtime::Error) -> String {
    let display = format!("{}", err);
    let root = err.root_cause().to_string();
    if display.contains(&root) {
        display
    } else {
        format!("{display}\n\n{root}")
    }
}

/// Classify an instantiation error as a runtime-dead `SetupError`.
/// Instantiation runs during `from_path` construction, before any
/// invocation — every such failure is a construction setup fault, not a
/// per-invocation cap outcome. The memory cap is dormant during
/// instantiation (see `Invocation::arm_memory_cap` /
/// `Invocation::disarm_memory_cap`) and the epoch deadline is not yet
/// armed, so the `trap_from` trap-class split does not apply here.
pub(crate) fn instantiate_err(err: wasmtime::Error) -> SetupError {
    SetupError::Dead(format!("instantiate: {err}"))
}

#[cfg(test)]
mod tests {
    use super::{other_trap_message, trap_from, NO_TIMEOUT_EPOCH_DELTA};
    use crate::invocation::{Invocation, MemoryLimitTrap, TimeoutTrap};
    use kobako_runtime::error::Trap;

    // The no-timeout priming delta is added to the engine's current
    // epoch inside wasmtime, and the process-wide ticker advances that
    // epoch from the first `shared_engine` call on — so the sum must
    // stay in range for a long-lived engine, not just a fresh one.
    // `increment_epoch` stands in for the ticker to make the ticked
    // state deterministic; under debug overflow checks an overflowing
    // delta panics right here.
    #[test]
    fn no_timeout_delta_survives_a_ticked_engine_epoch() {
        let engine = crate::cache::shared_engine().expect("shared engine must be constructible");
        engine.increment_epoch();
        let mut store = wasmtime::Store::new(engine, Invocation::new(None));
        store.set_epoch_deadline(NO_TIMEOUT_EPOCH_DELTA);
    }

    #[test]
    fn trap_from_routes_timeout_trap_to_timeout() {
        let err = wasmtime::Error::new(TimeoutTrap);
        let expected = TimeoutTrap.to_string();
        assert!(matches!(trap_from(err), Trap::Timeout(msg) if msg == expected));
    }

    #[test]
    fn trap_from_routes_memory_limit_trap_to_memory_limit() {
        let trap = MemoryLimitTrap::new(1 << 20, 1 << 19);
        let expected = trap.to_string();
        let err = wasmtime::Error::new(trap);
        assert!(matches!(trap_from(err), Trap::MemoryLimit(msg) if msg == expected));
    }

    #[test]
    fn trap_from_falls_back_to_other_for_unknown_errors() {
        let err = wasmtime::Error::msg("some other wasmtime fault");
        assert!(matches!(trap_from(err), Trap::Other(_)));
    }

    // A guest hard trap reaches the host as a wasmtime error whose Display is
    // only the backtrace framing, with the trap reason buried as the chain's
    // root cause. The named-capture regex bug surfaced as exactly this shape.
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
    #[test]
    fn other_trap_message_does_not_duplicate_a_flat_error() {
        let err = wasmtime::Error::msg("plain fault");
        assert_eq!(other_trap_message(&err), "plain fault");
    }
}
