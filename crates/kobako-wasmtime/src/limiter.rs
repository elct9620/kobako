//! The per-invocation memory cap: only the `memory.grow` delta past the
//! linear-memory size captured at invocation entry is charged, so the
//! image's initial allocation is outside the budget.

use wasmtime::ResourceLimiter;

/// Resource limiter that enforces the per-invocation `memory_limit`
/// cap.
///
/// `max_memory` is the byte cap on per-invocation growth (`None` disables
/// the cap). `baseline` is the linear-memory size captured at invocation
/// entry by `MemoryLimiter::activate`; the limiter charges only the
/// `memory.grow` delta past `baseline` against `max_memory`, so the
/// mruby image's initial allocation and any high-water mark left by
/// prior invocations on the same Sandbox do not consume the budget.
/// `cap_active` gates whether the cap is enforced — wasmtime's
/// `ResourceLimiter` also fires for the module's declared initial
/// allocation at instantiation time, but the cap stays dormant until
/// `MemoryLimiter::activate` flips the flag for one `Driver` invoke
/// call. When `cap_active` is `false`, the limiter always allows
/// growth.
///
/// When `memory.grow` would push the per-invocation delta past
/// `max_memory`, the limiter returns `MemoryLimitTrap` from
/// `memory_growing`; wasmtime turns that into the trap surfaced to the
/// host as a guest invocation failure.
#[derive(Debug, Clone, Copy)]
pub(crate) struct MemoryLimiter {
    max_memory: Option<usize>,
    baseline: usize,
    cap_active: bool,
    peak: usize,
}

impl MemoryLimiter {
    pub(crate) fn new(max_memory: Option<usize>) -> Self {
        Self {
            max_memory,
            baseline: 0,
            cap_active: false,
            peak: 0,
        }
    }

    /// The cap stays dormant until armed, because the module's declared
    /// initial memory is allocated during instantiation and must pass.
    pub(crate) fn activate(&mut self, baseline: usize) {
        self.baseline = baseline;
        self.cap_active = true;
        self.peak = 0;
    }

    pub(crate) fn deactivate(&mut self) {
        self.cap_active = false;
    }

    /// A grow the cap rejects never updates the peak, so the reported value
    /// never exceeds `memory_limit`.
    pub(crate) fn peak(&self) -> usize {
        self.peak
    }
}

impl ResourceLimiter for MemoryLimiter {
    fn memory_growing(
        &mut self,
        _current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        if !self.cap_active {
            return Ok(true);
        }
        let delta = desired.saturating_sub(self.baseline);
        if let Some(limit) = self.max_memory {
            if delta > limit {
                return Err(wasmtime::Error::new(MemoryLimitTrap { desired, limit }));
            }
        }
        if delta > self.peak {
            self.peak = delta;
        }
        Ok(true)
    }

    fn table_growing(
        &mut self,
        _current: usize,
        _desired: usize,
        _maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        Ok(true)
    }
}

/// Marker error returned from `MemoryLimiter::memory_growing` when the
/// per-invocation memory cap is exceeded. Downcast from the wasmtime
/// trap error to classify the failure as a memory-limit `Trap`. Callers
/// use the `Display` impl below — no field is read directly — so the
/// inner state stays private.
#[derive(Debug)]
pub(crate) struct MemoryLimitTrap {
    desired: usize,
    limit: usize,
}

impl MemoryLimitTrap {
    #[cfg(test)]
    pub(crate) fn new(desired: usize, limit: usize) -> Self {
        Self { desired, limit }
    }
}

impl std::fmt::Display for MemoryLimitTrap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "memory usage exceeded memory_limit: \
             requested={} bytes, limit={} bytes",
            self.desired, self.limit
        )
    }
}

impl std::error::Error for MemoryLimitTrap {}

#[cfg(test)]
mod tests {
    //! Unit tests for `MemoryLimiter` — the per-invocation memory
    //! delta cap. The Ruby-facing E2E suite exercises the full path
    //! through wasmtime; these tests pin the pure delta arithmetic so
    //! a regression that breaks the baseline accounting (e.g. dropping
    //! the `baseline` subtraction, or letting `activate` carry stale
    //! state across invocations) is caught without spinning up a
    //! Store.
    use super::{MemoryLimitTrap, MemoryLimiter};
    use wasmtime::ResourceLimiter;

    fn assert_growing(limiter: &mut MemoryLimiter, desired: usize) {
        assert!(
            limiter.memory_growing(0, desired, None).unwrap(),
            "expected memory_growing({desired}) to allow growth"
        );
    }

    fn assert_trapping(limiter: &mut MemoryLimiter, desired: usize) {
        let err = limiter
            .memory_growing(0, desired, None)
            .expect_err("expected memory_growing to trap");
        assert!(
            err.downcast_ref::<MemoryLimitTrap>().is_some(),
            "expected MemoryLimitTrap, got {err:?}"
        );
    }

    // @behavior S-113
    #[test]
    fn dormant_limiter_allows_any_growth() {
        let mut limiter = MemoryLimiter::new(Some(1 << 20));
        // Without `activate`, the cap is dormant — the module's
        // declared initial allocation must pass through unconditionally.
        assert_growing(&mut limiter, 100 << 20);
    }

    // @behavior S-114
    #[test]
    fn delta_below_cap_passes_after_activate() {
        let mut limiter = MemoryLimiter::new(Some(1 << 20));
        limiter.activate(2 << 20);
        // baseline=2 MiB, desired=2.5 MiB → delta=0.5 MiB ≤ 1 MiB cap.
        assert_growing(&mut limiter, (2 << 20) + (1 << 19));
    }

    // @behavior S-121
    #[test]
    fn delta_past_cap_traps_with_memory_limit_trap() {
        let mut limiter = MemoryLimiter::new(Some(1 << 20));
        limiter.activate(2 << 20);
        // baseline=2 MiB, desired=4 MiB → delta=2 MiB > 1 MiB cap.
        assert_trapping(&mut limiter, 4 << 20);
    }

    // @behavior S-008
    #[test]
    fn activate_resets_baseline_on_each_invocation() {
        let mut limiter = MemoryLimiter::new(Some(1 << 20));
        limiter.activate(2 << 20);
        assert_growing(&mut limiter, (2 << 20) + (1 << 20));
        // Second invocation: linear memory has grown to 3 MiB. Re-arming
        // must re-anchor the baseline so the next 1 MiB of growth fits
        // the per-invocation budget rather than being charged against
        // the prior invocation's residue.
        limiter.activate(3 << 20);
        assert_growing(&mut limiter, (3 << 20) + (1 << 20));
    }

    // @behavior S-009
    #[test]
    fn disabled_cap_ignores_delta_size() {
        let mut limiter = MemoryLimiter::new(None);
        limiter.activate(0);
        assert_growing(&mut limiter, 100 << 20);
    }

    // @behavior S-116
    #[test]
    fn peak_starts_at_zero_before_any_grow() {
        let limiter = MemoryLimiter::new(Some(1 << 20));
        assert_eq!(limiter.peak(), 0);
    }

    // @behavior S-115
    #[test]
    fn peak_tracks_high_water_of_delta_past_baseline() {
        let mut limiter = MemoryLimiter::new(Some(1 << 20));
        limiter.activate(2 << 20);
        assert_growing(&mut limiter, (2 << 20) + (1 << 18)); // delta=256 KiB
        assert_growing(&mut limiter, (2 << 20) + (1 << 19)); // delta=512 KiB (new peak)
        assert_growing(&mut limiter, (2 << 20) + (1 << 17)); // delta=128 KiB (below peak)
        assert_eq!(limiter.peak(), 1 << 19);
    }

    // @behavior S-062
    #[test]
    fn trap_does_not_update_peak() {
        let mut limiter = MemoryLimiter::new(Some(1 << 20));
        limiter.activate(2 << 20);
        assert_growing(&mut limiter, (2 << 20) + (1 << 19)); // delta=512 KiB
        assert_trapping(&mut limiter, (2 << 20) + (2 << 20)); // would be 2 MiB > 1 MiB cap
                                                              // Peak reflects the last accepted grow, not the rejected desired.
        assert_eq!(limiter.peak(), 1 << 19);
    }

    // @behavior S-117
    #[test]
    fn activate_resets_peak_for_new_invocation() {
        let mut limiter = MemoryLimiter::new(Some(1 << 20));
        limiter.activate(2 << 20);
        assert_growing(&mut limiter, (2 << 20) + (1 << 19));
        assert_eq!(limiter.peak(), 1 << 19);
        limiter.activate(3 << 20);
        assert_eq!(limiter.peak(), 0);
    }

    // @behavior S-118
    #[test]
    fn disabled_cap_still_tracks_peak() {
        let mut limiter = MemoryLimiter::new(None);
        limiter.activate(1 << 20);
        assert_growing(&mut limiter, (1 << 20) + (4 << 20));
        assert_eq!(limiter.peak(), 4 << 20);
    }
}
