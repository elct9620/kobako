//! Trap classification for the run path.
//!
//! Maps a `wasmtime` run error to the right top-level `Kobako::*` Ruby
//! exception (`TimeoutError` / `MemoryLimitError` / `TrapError`), and
//! hosts the epoch-deadline callback that raises the wall-clock
//! `TimeoutTrap`. The classification is a pure function over the error's
//! downcast chain so it can be exercised from `cargo test` without the
//! magnus surface; the trap marker types themselves live in
//! `super::invocation` (where the limiter / callback construct them).

use std::time::Instant;

use magnus::{Error as MagnusError, Ruby};
use wasmtime::{StoreContextMut, UpdateDeadline};

use super::errors::{memory_limit_err, setup_err, timeout_err, trap_err};
use super::invocation::{Invocation, MemoryLimitTrap, TimeoutTrap};

/// Epoch-deadline callback installed on every Store. Read the per-run
/// wall-clock deadline from `Invocation` and trap with
/// `TimeoutTrap` once the deadline has passed; otherwise extend the
/// next check by one tick of the process-wide epoch ticker. When the
/// deadline is `None` the callback should not fire under normal
/// `Runtime::eval` / `Runtime::run` flow because
/// `set_epoch_deadline(u64::MAX)` is used; returning a long extension
/// keeps the callback inert as a defence in depth.
pub(super) fn epoch_deadline_callback(
    ctx: StoreContextMut<'_, Invocation>,
) -> wasmtime::Result<UpdateDeadline> {
    match ctx.data().deadline() {
        Some(deadline) if Instant::now() >= deadline => Err(wasmtime::Error::new(TimeoutTrap)),
        Some(_) => Ok(UpdateDeadline::Continue(1)),
        None => Ok(UpdateDeadline::Continue(u64::MAX / 2)),
    }
}

/// Configured-cap path classification for a wasmtime error. The
/// downcast logic stays in a pure helper so the
/// `Kobako::TimeoutError` / `MemoryLimitError` /
/// `Kobako::TrapError` mapping can be exercised from `cargo test`
/// without the magnus surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrapClass {
    /// Wall-clock cap path.
    Timeout,
    /// Linear-memory cap path.
    MemoryLimit,
    /// Any other wasmtime error — surfaces as the base
    /// `Kobako::TrapError`.
    Other,
}

/// Inspect a wasmtime error to decide which top-level `Kobako::*` trap
/// class it should map to. Pure function — operates on the error's
/// downcast chain only, no magnus / Ruby state required.
fn classify_trap(err: &wasmtime::Error) -> TrapClass {
    if err.downcast_ref::<TimeoutTrap>().is_some() {
        TrapClass::Timeout
    } else if err.downcast_ref::<MemoryLimitTrap>().is_some() {
        TrapClass::MemoryLimit
    } else {
        TrapClass::Other
    }
}

/// Map a wasmtime call error to the right top-level `Kobako::*` Ruby
/// exception class. The ABI export symbol (`__kobako_eval` /
/// `__kobako_run`) is deliberately omitted from the message — the
/// Sandbox layer attaches the user-facing verb (`Sandbox#eval` /
/// `Sandbox#run`) so the message reads in caller vocabulary rather
/// than ABI vocabulary.
///
/// For the configured-cap paths (`TrapClass::Timeout` /
/// `TrapClass::MemoryLimit`) the trap's own `std::fmt::Display`
/// carries the user-facing reason (`"wall-clock deadline exceeded"`,
/// `"linear memory growth exceeded memory_limit: ..."`). The wasmtime
/// outer wrapper at `format!("{}", err)` would otherwise surface only
/// the `"error while executing at wasm backtrace: ..."` framing, which
/// is operator noise on a cap trap. For `TrapClass::Other` the framing
/// is kept but the chain's root cause is appended (see
/// `other_trap_message`) so the real trap reason survives.
pub(super) fn call_err(ruby: &Ruby, err: wasmtime::Error) -> MagnusError {
    match classify_trap(&err) {
        TrapClass::Timeout => {
            let msg = err
                .downcast_ref::<TimeoutTrap>()
                .map(|t| t.to_string())
                .unwrap_or_else(|| format!("{}", err));
            timeout_err(ruby, msg)
        }
        TrapClass::MemoryLimit => {
            let msg = err
                .downcast_ref::<MemoryLimitTrap>()
                .map(|t| t.to_string())
                .unwrap_or_else(|| format!("{}", err));
            memory_limit_err(ruby, msg)
        }
        TrapClass::Other => trap_err(ruby, other_trap_message(&err)),
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

/// Map an instantiation error to `Kobako::SetupError`. Instantiation runs
/// during `from_path` construction, before any invocation — every such
/// failure is a construction setup fault, not a
/// per-invocation cap outcome. The memory cap is dormant during
/// instantiation (see `Invocation::arm_memory_cap` /
/// `Invocation::disarm_memory_cap`) and the epoch deadline is not yet
/// armed, so the `call_err` trap-class split does not apply here.
pub(super) fn instantiate_err(ruby: &Ruby, err: wasmtime::Error) -> MagnusError {
    setup_err(ruby, format!("instantiate: {}", err))
}

#[cfg(test)]
mod tests {
    use super::{classify_trap, other_trap_message, TrapClass};
    use crate::runtime::invocation::{MemoryLimitTrap, TimeoutTrap};

    #[test]
    fn classify_trap_routes_timeout_trap_to_timeout() {
        let err = wasmtime::Error::new(TimeoutTrap);
        assert_eq!(classify_trap(&err), TrapClass::Timeout);
    }

    #[test]
    fn classify_trap_routes_memory_limit_trap_to_memory_limit() {
        let err = wasmtime::Error::new(MemoryLimitTrap::new(1 << 20, 1 << 19));
        assert_eq!(classify_trap(&err), TrapClass::MemoryLimit);
    }

    #[test]
    fn classify_trap_falls_back_to_other_for_unknown_errors() {
        let err = wasmtime::Error::msg("some other wasmtime fault");
        assert_eq!(classify_trap(&err), TrapClass::Other);
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
