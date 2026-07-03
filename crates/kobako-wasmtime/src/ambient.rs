//! Denial of guest ambient authority at the WASI layer — the one grant
//! that separates the hermetic rung from permissive;
//! `frames::install_wasi_frames` wires these sources on `Hermetic` only.
//!
//! `wasmtime-wasi`'s `WasiCtxBuilder` defaults the guest's `wasi:clocks` to
//! the host wall / monotonic clock and `wasi:random` to a fresh per-context
//! seed. No allowlisted mrbgem reaches these preview1 imports today
//! (`build_config/wasi.rb`), but a future libc-backed gem would silently
//! obtain real time and host entropy — a covert timing channel and a
//! nondeterminism source the hermetic posture deliberately excludes
//! (docs/security-model.md).
//! Pinning the clocks to the Unix epoch and the RNG to a constant stream
//! makes that denial a property of the host, not merely of the gem allowlist.
//!
//! The host wall-clock cap is unaffected: the per-invocation timeout runs on
//! wasmtime epoch interruption against a host `Instant`, never the guest's
//! frozen `wasi:clocks/monotonic-clock`.

use std::time::Duration;

use wasmtime_wasi::random::Deterministic;
use wasmtime_wasi::{HostMonotonicClock, HostWallClock};

/// Wall clock frozen at the Unix epoch — the guest observes no real time.
pub(crate) struct FrozenWallClock;

impl HostWallClock for FrozenWallClock {
    fn resolution(&self) -> Duration {
        Duration::from_secs(1)
    }

    fn now(&self) -> Duration {
        Duration::ZERO
    }
}

/// Monotonic clock frozen at zero — the guest observes no elapsed time.
pub(crate) struct FrozenMonotonicClock;

impl HostMonotonicClock for FrozenMonotonicClock {
    fn resolution(&self) -> u64 {
        1
    }

    fn now(&self) -> u64 {
        0
    }
}

/// Constant-stream RNG handed to the guest's `wasi:random`, so a guest that
/// reaches `random_get` receives no host entropy.
pub(crate) fn deterministic_rng() -> Deterministic {
    Deterministic::new(vec![0])
}

#[cfg(test)]
mod tests {
    //! Pin the frozen-clock contract: the guest must observe no real wall
    //! or monotonic time through `wasi:clocks`, regardless of the host's
    //! actual clock. A regression that reverts either clock to the WASI
    //! default would re-expose ambient time to a future libc-backed gem.
    use super::*;

    #[test]
    fn the_guest_wall_clock_reads_the_unix_epoch() {
        assert_eq!(
            FrozenWallClock.now(),
            Duration::ZERO,
            "guest wasi:clocks/wall-clock must read the Unix epoch, not host time"
        );
    }

    #[test]
    fn the_guest_monotonic_clock_never_advances() {
        assert_eq!(
            FrozenMonotonicClock.now(),
            0,
            "guest wasi:clocks/monotonic-clock must stay frozen at zero"
        );
    }
}
