//! The per-invocation memory cap: only the growth past the linear-memory size
//! captured at invocation entry is charged, so the image's initial allocation
//! is outside the budget.

/// The budget one invocation's memory grows against.
///
/// `max_memory` is the byte cap on per-invocation growth (`None` disables
/// the cap). `baseline` is the linear-memory size captured at invocation
/// entry by `MemoryCap::activate`; only the growth past `baseline` is charged
/// against `max_memory`, so the mruby image's initial allocation and any
/// high-water mark left by prior invocations on the same Sandbox do not
/// consume the budget. An Engine also asks about the module's declared
/// initial allocation at instantiation time, so the cap stays dormant until
/// `MemoryCap::activate` arms it for one invocation.
#[derive(Debug, Clone, Copy)]
pub struct MemoryCap {
    max_memory: Option<usize>,
    baseline: usize,
    cap_active: bool,
    peak: usize,
}

impl MemoryCap {
    pub fn new(max_memory: Option<usize>) -> Self {
        Self {
            max_memory,
            baseline: 0,
            cap_active: false,
            peak: 0,
        }
    }

    /// The cap stays dormant until armed, because the module's declared
    /// initial memory is allocated during instantiation and must pass.
    pub fn activate(&mut self, baseline: usize) {
        self.baseline = baseline;
        self.cap_active = true;
        self.peak = 0;
    }

    pub fn deactivate(&mut self) {
        self.cap_active = false;
    }

    /// A grow the cap rejects never updates the peak, so the reported value
    /// never exceeds `memory_limit`.
    pub fn peak(&self) -> usize {
        self.peak
    }

    /// Charge a growth to `desired` bytes, refusing one past the cap.
    pub fn growing(&mut self, desired: usize) -> Result<(), MemoryLimitExceeded> {
        if !self.cap_active {
            return Ok(());
        }
        let delta = desired.saturating_sub(self.baseline);
        if let Some(limit) = self.max_memory {
            if delta > limit {
                return Err(MemoryLimitExceeded::new(desired, limit));
            }
        }
        if delta > self.peak {
            self.peak = delta;
        }
        Ok(())
    }
}

/// A growth past the per-invocation memory cap. An Engine ends the invocation
/// with it, and a Driver finds it again to classify the failure as a
/// memory-limit `Trap`. Callers use the `Display` impl below — no field is
/// read directly — so the inner state stays private.
#[derive(Debug)]
pub struct MemoryLimitExceeded {
    desired: usize,
    limit: usize,
}

impl MemoryLimitExceeded {
    pub fn new(desired: usize, limit: usize) -> Self {
        Self { desired, limit }
    }
}

impl std::fmt::Display for MemoryLimitExceeded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "memory usage exceeded memory_limit: \
             requested={} bytes, limit={} bytes",
            self.desired, self.limit
        )
    }
}

impl std::error::Error for MemoryLimitExceeded {}

#[cfg(test)]
mod tests {
    //! Unit tests for `MemoryCap` — the per-invocation memory delta cap. The
    //! Ruby-facing E2E suite exercises the full path through an Engine; these
    //! tests pin the pure delta arithmetic so a regression that breaks the
    //! baseline accounting (e.g. dropping the `baseline` subtraction, or
    //! letting `activate` carry stale state across invocations) is caught
    //! without spinning one up.
    use super::MemoryCap;

    fn assert_growing(cap: &mut MemoryCap, desired: usize) {
        assert!(
            cap.growing(desired).is_ok(),
            "expected growing({desired}) to allow growth"
        );
    }

    fn assert_trapping(cap: &mut MemoryCap, desired: usize) {
        assert!(
            cap.growing(desired).is_err(),
            "expected growing({desired}) to refuse growth"
        );
    }

    // @behavior S-113
    #[test]
    fn dormant_limiter_allows_any_growth() {
        let mut cap = MemoryCap::new(Some(1 << 20));
        // Without `activate`, the cap is dormant — the module's
        // declared initial allocation must pass through unconditionally.
        assert_growing(&mut cap, 100 << 20);
    }

    // @behavior S-114
    #[test]
    fn delta_below_cap_passes_after_activate() {
        let mut cap = MemoryCap::new(Some(1 << 20));
        cap.activate(2 << 20);
        // baseline=2 MiB, desired=2.5 MiB → delta=0.5 MiB ≤ 1 MiB cap.
        assert_growing(&mut cap, (2 << 20) + (1 << 19));
    }

    // @behavior S-121
    #[test]
    fn delta_past_cap_traps_with_memory_limit_trap() {
        let mut cap = MemoryCap::new(Some(1 << 20));
        cap.activate(2 << 20);
        // baseline=2 MiB, desired=4 MiB → delta=2 MiB > 1 MiB cap.
        assert_trapping(&mut cap, 4 << 20);
    }

    // @behavior S-008
    #[test]
    fn activate_resets_baseline_on_each_invocation() {
        let mut cap = MemoryCap::new(Some(1 << 20));
        cap.activate(2 << 20);
        assert_growing(&mut cap, (2 << 20) + (1 << 20));
        // Second invocation: linear memory has grown to 3 MiB. Re-arming
        // must re-anchor the baseline so the next 1 MiB of growth fits
        // the per-invocation budget rather than being charged against
        // the prior invocation's residue.
        cap.activate(3 << 20);
        assert_growing(&mut cap, (3 << 20) + (1 << 20));
    }

    // @behavior S-009
    #[test]
    fn disabled_cap_ignores_delta_size() {
        let mut cap = MemoryCap::new(None);
        cap.activate(0);
        assert_growing(&mut cap, 100 << 20);
    }

    // @behavior S-116
    #[test]
    fn peak_starts_at_zero_before_any_grow() {
        let cap = MemoryCap::new(Some(1 << 20));
        assert_eq!(cap.peak(), 0);
    }

    // @behavior S-115
    #[test]
    fn peak_tracks_high_water_of_delta_past_baseline() {
        let mut cap = MemoryCap::new(Some(1 << 20));
        cap.activate(2 << 20);
        assert_growing(&mut cap, (2 << 20) + (1 << 18)); // delta=256 KiB
        assert_growing(&mut cap, (2 << 20) + (1 << 19)); // delta=512 KiB (new peak)
        assert_growing(&mut cap, (2 << 20) + (1 << 17)); // delta=128 KiB (below peak)
        assert_eq!(cap.peak(), 1 << 19);
    }

    // @behavior S-062
    #[test]
    fn trap_does_not_update_peak() {
        let mut cap = MemoryCap::new(Some(1 << 20));
        cap.activate(2 << 20);
        assert_growing(&mut cap, (2 << 20) + (1 << 19)); // delta=512 KiB
        assert_trapping(&mut cap, (2 << 20) + (2 << 20)); // would be 2 MiB > 1 MiB cap
                                                          // Peak reflects the last accepted grow, not the rejected desired.
        assert_eq!(cap.peak(), 1 << 19);
    }

    // @behavior S-117
    #[test]
    fn activate_resets_peak_for_new_invocation() {
        let mut cap = MemoryCap::new(Some(1 << 20));
        cap.activate(2 << 20);
        assert_growing(&mut cap, (2 << 20) + (1 << 19));
        assert_eq!(cap.peak(), 1 << 19);
        cap.activate(3 << 20);
        assert_eq!(cap.peak(), 0);
    }

    // @behavior S-118
    #[test]
    fn disabled_cap_still_tracks_peak() {
        let mut cap = MemoryCap::new(None);
        cap.activate(1 << 20);
        assert_growing(&mut cap, (1 << 20) + (4 << 20));
        assert_eq!(cap.peak(), 4 << 20);
    }
}
