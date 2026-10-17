//! The guest's time and entropy at the WASI layer — the one ambient
//! authority that separates the hermetic rung from permissive. `Wasi::new`
//! wires the frozen sources on `Hermetic` and the live clock on
//! `Permissive`; `Clocks` is what the guest's waits pass on under either.
//!
//! `wasmtime-wasi`'s `WasiCtxBuilder` defaults the guest's `wasi:clocks` to
//! the host wall / monotonic clock and `wasi:random` to a fresh per-context
//! seed. No allowlisted mrbgem reaches these preview1 imports today
//! (`build_config/wasi.rb`), but a future libc-backed gem would silently
//! obtain real time and host entropy — a covert timing channel and a
//! nondeterminism source the hermetic posture deliberately excludes
//! (docs/guides/security.md).
//! Pinning the clocks to the Unix epoch and the RNG to a constant stream
//! makes that denial a property of the host, not merely of the gem allowlist.
//!
//! The host wall-clock cap is unaffected: the per-invocation timeout runs on
//! wasmtime epoch interruption against a host `Instant`, never the guest's
//! frozen `wasi:clocks/monotonic-clock`.

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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

/// The host's monotonic clock read from a fixed origin. The guest's
/// `clock_time_get` and a wait it subscribes to share it, so both agree on
/// what an instant means.
#[derive(Clone, Copy)]
pub(crate) struct LiveMonotonicClock {
    origin: Instant,
}

impl LiveMonotonicClock {
    pub(crate) fn new() -> Self {
        Self {
            origin: Instant::now(),
        }
    }
}

impl HostMonotonicClock for LiveMonotonicClock {
    fn resolution(&self) -> u64 {
        1
    }

    fn now(&self) -> u64 {
        u64::try_from(self.origin.elapsed().as_nanos()).unwrap_or(u64::MAX)
    }
}

/// The clocks the guest observes. A wait it subscribes to passes on the
/// same clocks: frozen ones never advance, so the wait ends at once.
#[derive(Clone, Copy)]
pub(crate) enum Clocks {
    Frozen,
    Live(LiveMonotonicClock),
}

impl Clocks {
    pub(crate) fn monotonic_now(&self) -> u64 {
        match self {
            Self::Frozen => FrozenMonotonicClock.now(),
            Self::Live(clock) => clock.now(),
        }
    }

    pub(crate) fn wall_now(&self) -> u64 {
        let since_epoch = match self {
            Self::Frozen => FrozenWallClock.now(),
            Self::Live(_) => SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or(Duration::ZERO),
        };
        u64::try_from(since_epoch.as_nanos()).unwrap_or(u64::MAX)
    }

    /// Whether a wait on these clocks takes host time.
    pub(crate) fn advances(&self) -> bool {
        matches!(self, Self::Live(_))
    }
}

#[cfg(test)]
mod tests {
    //! Pin the frozen-clock contract: the guest must observe no real wall
    //! or monotonic time through `wasi:clocks`, regardless of the host's
    //! actual clock. A regression that reverts either clock to the WASI
    //! default would re-expose ambient time to a future libc-backed gem.
    use super::*;

    // @behavior RT-047
    #[test]
    fn the_guest_wall_clock_reads_the_unix_epoch() {
        assert_eq!(
            FrozenWallClock.now(),
            Duration::ZERO,
            "guest wasi:clocks/wall-clock must read the Unix epoch, not host time"
        );
    }

    // @behavior RT-051
    #[test]
    fn the_guest_monotonic_clock_never_advances() {
        assert_eq!(
            FrozenMonotonicClock.now(),
            0,
            "guest wasi:clocks/monotonic-clock must stay frozen at zero"
        );
    }
}
