//! The per-invocation memory cap: only the growth past the linear-memory size
//! captured at invocation entry is charged, so the image's initial allocation
//! is outside the budget.

/// The budget one invocation's memory grows against.
///
/// `memory_limit` bounds per-invocation growth (`None` disables it).
/// `baseline` is the linear-memory size captured at invocation entry by
/// `MemoryCap::activate`; only the growth past `baseline` is charged against
/// `memory_limit`, so the mruby image's initial allocation and any high-water
/// mark left by prior invocations on the same Sandbox do not consume the
/// budget. An Engine also asks about the module's declared initial allocation
/// at instantiation time, so the cap stays dormant until `MemoryCap::activate`
/// arms it for one invocation.
#[derive(Debug, Clone, Copy)]
pub struct MemoryCap {
    memory_limit: Option<usize>,
    baseline: usize,
    cap_active: bool,
    peak: usize,
}

impl MemoryCap {
    pub fn new(memory_limit: Option<usize>) -> Self {
        Self {
            memory_limit,
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

    /// Charge a growth to `desired` bytes, refusing one past `memory_limit`.
    pub fn charge(&mut self, desired: usize) -> Result<(), MemoryLimitExceeded> {
        if !self.cap_active {
            return Ok(());
        }
        let delta = desired.saturating_sub(self.baseline);
        if let Some(limit) = self.memory_limit {
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
/// with it, and a Wasm Driver finds it again to classify the failure as a
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
    //! The pure delta arithmetic, so a regression that breaks the baseline
    //! accounting (dropping the `baseline` subtraction, or letting `activate`
    //! carry stale state across invocations) is caught without an Engine.
    use super::MemoryCap;

    fn assert_charged(cap: &mut MemoryCap, desired: usize, what: &str) {
        assert!(
            cap.charge(desired).is_ok(),
            "{what} through MemoryCap::charge must be allowed"
        );
    }

    fn assert_refused(cap: &mut MemoryCap, desired: usize, what: &str) {
        assert!(
            cap.charge(desired).is_err(),
            "{what} through MemoryCap::charge must be refused"
        );
    }

    // @behavior S-113
    #[test]
    fn a_growth_before_the_cap_is_armed_is_not_bounded() {
        let mut cap = MemoryCap::new(Some(1 << 20));
        assert_charged(&mut cap, 100 << 20, "a 100 MiB growth before activate");
    }

    // @behavior S-114
    #[test]
    fn a_growth_within_the_limit_is_allowed() {
        let mut cap = MemoryCap::new(Some(1 << 20));
        cap.activate(2 << 20);
        assert_charged(
            &mut cap,
            (2 << 20) + (1 << 19),
            "a 512 KiB growth past a 2 MiB baseline under a 1 MiB limit",
        );
    }

    // @behavior S-121
    #[test]
    fn a_growth_past_the_limit_is_refused() {
        let mut cap = MemoryCap::new(Some(1 << 20));
        cap.activate(2 << 20);
        assert_refused(
            &mut cap,
            4 << 20,
            "a 2 MiB growth past a 2 MiB baseline under a 1 MiB limit",
        );
    }

    // @behavior S-008
    #[test]
    fn each_activation_charges_from_its_own_baseline() {
        let mut cap = MemoryCap::new(Some(1 << 20));
        cap.activate(2 << 20);
        assert_charged(&mut cap, 3 << 20, "a 1 MiB growth in the first invocation");
        cap.activate(3 << 20);
        assert_charged(
            &mut cap,
            4 << 20,
            "a 1 MiB growth in the second invocation, past the first one's residue",
        );
    }

    // @behavior S-009
    #[test]
    fn a_cap_without_a_limit_allows_any_growth() {
        let mut cap = MemoryCap::new(None);
        cap.activate(0);
        assert_charged(&mut cap, 100 << 20, "a 100 MiB growth with no limit");
    }

    // @behavior S-116
    #[test]
    fn the_peak_is_zero_before_any_growth() {
        let cap = MemoryCap::new(Some(1 << 20));
        assert_eq!(
            cap.peak(),
            0,
            "a new cap through MemoryCap::peak must report 0"
        );
    }

    // @behavior S-115
    #[test]
    fn the_peak_is_the_highest_growth_past_the_baseline() {
        let mut cap = MemoryCap::new(Some(1 << 20));
        cap.activate(2 << 20);
        assert_charged(&mut cap, (2 << 20) + (1 << 18), "a 256 KiB growth");
        assert_charged(&mut cap, (2 << 20) + (1 << 19), "a 512 KiB growth");
        assert_charged(&mut cap, (2 << 20) + (1 << 17), "a 128 KiB growth");
        assert_eq!(
            cap.peak(),
            1 << 19,
            "growths of 256, 512, and 128 KiB through MemoryCap::peak must report 512 KiB"
        );
    }

    // @behavior S-062
    #[test]
    fn a_refused_growth_leaves_the_peak_alone() {
        let mut cap = MemoryCap::new(Some(1 << 20));
        cap.activate(2 << 20);
        assert_charged(&mut cap, (2 << 20) + (1 << 19), "a 512 KiB growth");
        assert_refused(&mut cap, 4 << 20, "a 2 MiB growth under a 1 MiB limit");
        assert_eq!(
            cap.peak(),
            1 << 19,
            "a refused 2 MiB growth through MemoryCap::peak must leave 512 KiB reported"
        );
    }

    // @behavior S-117
    #[test]
    fn each_activation_resets_the_peak() {
        let mut cap = MemoryCap::new(Some(1 << 20));
        cap.activate(2 << 20);
        assert_charged(&mut cap, (2 << 20) + (1 << 19), "a 512 KiB growth");
        cap.activate(3 << 20);
        assert_eq!(
            cap.peak(),
            0,
            "a second activate through MemoryCap::peak must report 0"
        );
    }

    // @behavior S-118
    #[test]
    fn a_cap_without_a_limit_still_reports_its_peak() {
        let mut cap = MemoryCap::new(None);
        cap.activate(1 << 20);
        assert_charged(&mut cap, 5 << 20, "a 4 MiB growth with no limit");
        assert_eq!(
            cap.peak(),
            4 << 20,
            "a 4 MiB growth with no limit through MemoryCap::peak must report 4 MiB"
        );
    }
}
