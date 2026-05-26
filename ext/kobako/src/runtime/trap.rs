//! Trap classification for the run path.
//!
//! Maps a `wasmtime` run error to the right top-level `Kobako::*` Ruby
//! exception (`TimeoutError` / `MemoryLimitError` / `TrapError`), and
//! hosts the epoch-deadline callback that raises the wall-clock
//! [`TimeoutTrap`]. The classification is a pure function over the error's
//! downcast chain so it can be exercised from `cargo test` without the
//! magnus surface; the trap marker types themselves live in
//! `super::invocation` (where the limiter / callback construct them).

use std::time::Instant;

use magnus::{Error as MagnusError, Ruby};
use wasmtime::{StoreContextMut, UpdateDeadline};

use super::invocation::{Invocation, MemoryLimitTrap, TimeoutTrap};
use super::{memory_limit_err, setup_err, timeout_err, trap_err};

/// Epoch-deadline callback installed on every Store. Read the per-run
/// wall-clock deadline from [`Invocation`] (docs/behavior.md B-01) and trap with
/// [`TimeoutTrap`] once the deadline has passed; otherwise extend the
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
    /// docs/behavior.md E-19 wall-clock cap path.
    Timeout,
    /// docs/behavior.md E-20 linear-memory cap path.
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
/// For the configured-cap paths ([`TrapClass::Timeout`] /
/// [`TrapClass::MemoryLimit`]) the trap's own [`std::fmt::Display`]
/// carries the user-facing reason (`"wall-clock deadline exceeded"`,
/// `"linear memory growth exceeded memory_limit: ..."`). The wasmtime
/// outer wrapper at `format!("{}", err)` would otherwise surface only
/// the `"error while executing at wasm backtrace: ..."` framing, which
/// is operator noise on a cap trap. For [`TrapClass::Other`] the
/// wasmtime wrapper IS the diagnostic (real script trap) so it stays.
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
        TrapClass::Other => trap_err(ruby, format!("{}", err)),
    }
}

/// Map an instantiation error to `Kobako::SetupError`. Instantiation runs
/// during `from_path` construction, before any invocation — docs/behavior.md
/// E-41 classifies every such failure as a construction setup fault, not a
/// per-invocation cap outcome. The memory cap is dormant during
/// instantiation (see [`Invocation::arm_memory_cap`] /
/// [`Invocation::disarm_memory_cap`]) and the epoch deadline is not yet
/// armed, so the [`call_err`] trap-class split does not apply here.
pub(super) fn instantiate_err(ruby: &Ruby, err: wasmtime::Error) -> MagnusError {
    setup_err(ruby, format!("instantiate: {}", err))
}

#[cfg(test)]
mod tests {
    use super::{classify_trap, TrapClass};
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
}
