//! The memory one Invocation's guest may take, as a virtual machine's RAM is
//! bounded apart from what its runtime holds for it: the guest's pages count
//! in one `memory.current` against `memory.max`, and what the host holds for
//! the guest is bounded by counts and fixed sizes instead.

use std::sync::{Arc, Mutex, PoisonError};

/// What one Invocation's guest has taken against its `memory_limit`: its
/// memories, tables, and heaps grown past what its Guest Binary declares.
/// `None` bounds nothing, while the growth is still counted so the peak is
/// reported either way.
pub struct MemoryBudget {
    limit: Option<usize>,
    charged: Mutex<Charged>,
}

#[derive(Default)]
struct Charged {
    /// Whether the guest's growth is charged; it is not before the run's
    /// Store opens, nor once the run returns.
    armed: bool,
    /// What the Guest Binary declares, which the growth that creates its
    /// memories and tables fills without charge.
    declared: usize,
    /// Every growth since the budget was armed, whichever memory, table, or
    /// heap it lands in.
    grown: usize,
    peak: usize,
}

impl Charged {
    fn total(&self) -> usize {
        self.grown.saturating_sub(self.declared)
    }
}

impl MemoryBudget {
    pub fn new(limit: Option<usize>) -> Arc<Self> {
        Arc::new(Self {
            limit,
            charged: Mutex::default(),
        })
    }

    /// The `memory_limit` the budget holds the run to; `None` lifts it.
    pub fn limit(&self) -> Option<usize> {
        self.limit
    }

    /// Charge the guest's growth from now on, as the run's Store opens, past
    /// the `declared` bytes its Guest Binary's memories and tables start
    /// with, so what the binary declares takes none of it.
    pub fn arm(&self, declared: usize) {
        let mut charged = self.charged();
        charged.armed = true;
        charged.declared = declared;
        charged.grown = 0;
        charged.peak = 0;
    }

    /// Stop charging the guest's growth once the run returns, so reading its
    /// answer back costs the script nothing.
    pub fn disarm(&self) {
        self.charged().armed = false;
    }

    /// The most the run held at once, never past `memory_limit`, since a
    /// growth the budget refused never raises it.
    pub fn peak(&self) -> usize {
        self.charged().peak
    }

    /// Charge `bytes` the guest's memory, table, or heap grows by, refused
    /// where they and what is already taken would pass the limit, as each
    /// page a cgroup's task faults in adds to one `memory.current`. What it
    /// answers gives the charge back should the growth then fail.
    pub fn charge_growth(&self, bytes: usize) -> Result<GrowthCharge, MemoryLimitExceeded> {
        let mut charged = self.charged();
        if !charged.armed {
            return Ok(GrowthCharge { bytes: 0 });
        }
        let before = charged.grown;
        charged.grown = before.saturating_add(bytes);
        let total = charged.total();
        if let Some(limit) = self.limit.filter(|&limit| total > limit) {
            charged.grown = before;
            return Err(MemoryLimitExceeded::new(total, limit));
        }
        charged.peak = charged.peak.max(total);
        Ok(GrowthCharge { bytes })
    }

    /// Give back `charge`, a growth the Engine failed to make once the budget
    /// allowed it, so what the run holds counts no memory it never had, as
    /// `page_counter_cancel` gives a charge back. The peak keeps it, as the
    /// watermark the charge raised stays.
    pub fn cancel_growth(&self, charge: GrowthCharge) {
        let mut charged = self.charged();
        charged.grown = charged.grown.saturating_sub(charge.bytes);
    }

    fn charged(&self) -> std::sync::MutexGuard<'_, Charged> {
        self.charged.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The bytes one growth charged, which a growth the Engine fails to make
/// gives back through `MemoryBudget::cancel_growth`.
#[must_use]
pub struct GrowthCharge {
    bytes: usize,
}

/// A growth past the per-invocation memory budget. An Engine ends the
/// invocation with it, and a Wasm Driver finds it again to classify the
/// failure as a memory-limit `Trap`. Callers use the `Display` impl below — no
/// field is read directly — so the inner state stays private.
#[derive(Clone, Debug)]
pub struct MemoryLimitExceeded {
    requested: usize,
    limit: usize,
}

impl MemoryLimitExceeded {
    pub fn new(requested: usize, limit: usize) -> Self {
        Self { requested, limit }
    }
}

impl std::fmt::Display for MemoryLimitExceeded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "memory usage exceeded memory_limit: \
             requested={} bytes, limit={} bytes",
            self.requested, self.limit
        )
    }
}

impl std::error::Error for MemoryLimitExceeded {}

#[cfg(test)]
mod tests {
    //! The arithmetic of one budget, so a regression that breaks the baseline
    //! (dropping its subtraction, or letting `arm` carry stale state across
    //! invocations) is caught without an Engine.
    use super::MemoryBudget;

    fn assert_charged(budget: &MemoryBudget, bytes: usize, what: &str) {
        assert!(
            budget.charge_growth(bytes).is_ok(),
            "{what} through MemoryBudget::charge_growth must be allowed"
        );
    }

    fn assert_refused(budget: &MemoryBudget, bytes: usize, what: &str) {
        assert!(
            budget.charge_growth(bytes).is_err(),
            "{what} through MemoryBudget::charge_growth must be refused"
        );
    }

    // @behavior S-113
    #[test]
    fn what_the_guest_binary_declares_takes_none_of_the_budget() {
        let budget = MemoryBudget::new(Some(1 << 20));
        budget.arm(2 << 20);
        assert_charged(&budget, 2 << 20, "creating 2 MiB a Guest Binary declares");
        assert_eq!(
            budget.peak(),
            0,
            "creating the 2 MiB a Guest Binary declares under a 1 MiB limit must leave \
             MemoryBudget::peak at 0"
        );
    }

    // @behavior S-256
    #[test]
    fn growths_of_apart_memories_add_up() {
        let budget = MemoryBudget::new(Some(1 << 20));
        budget.arm(0);
        assert_charged(&budget, 3 << 18, "a 768 KiB growth of one memory");
        assert_refused(
            &budget,
            3 << 18,
            "a 768 KiB growth of another memory beside it under a 1 MiB limit",
        );
    }

    // @behavior S-114
    #[test]
    fn a_growth_within_the_limit_is_allowed() {
        let budget = MemoryBudget::new(Some(1 << 20));
        budget.arm(2 << 20);
        assert_charged(
            &budget,
            (2 << 20) + (1 << 19),
            "a 512 KiB growth past a 2 MiB baseline under a 1 MiB limit",
        );
    }

    // @behavior S-121
    #[test]
    fn a_growth_past_the_limit_is_refused() {
        let budget = MemoryBudget::new(Some(1 << 20));
        budget.arm(2 << 20);
        assert_refused(
            &budget,
            4 << 20,
            "a 2 MiB growth past a 2 MiB baseline under a 1 MiB limit",
        );
    }

    // @behavior S-008
    #[test]
    fn each_arming_charges_from_its_own_baseline() {
        let budget = MemoryBudget::new(Some(1 << 20));
        budget.arm(2 << 20);
        assert_charged(&budget, 3 << 20, "a 1 MiB growth in the first invocation");
        budget.arm(3 << 20);
        assert_charged(
            &budget,
            4 << 20,
            "a 1 MiB growth in the second invocation, past the first one's residue",
        );
    }

    // @behavior S-009
    #[test]
    fn a_budget_without_a_limit_allows_any_growth() {
        let budget = MemoryBudget::new(None);
        budget.arm(0);
        assert_charged(&budget, 100 << 20, "a 100 MiB growth with no limit");
    }

    // @behavior S-116
    #[test]
    fn the_peak_is_zero_before_any_growth() {
        let budget = MemoryBudget::new(Some(1 << 20));
        assert_eq!(
            budget.peak(),
            0,
            "a new budget through MemoryBudget::peak must report 0"
        );
    }

    // @behavior S-115
    #[test]
    fn the_peak_is_the_most_held_at_once() {
        let budget = MemoryBudget::new(Some(1 << 20));
        budget.arm(2 << 20);
        assert_charged(&budget, (2 << 20) + (1 << 18), "a 256 KiB growth");
        let failed = budget
            .charge_growth(1 << 18)
            .expect("another 256 KiB growth within the limit must be allowed");
        budget.cancel_growth(failed);
        assert_eq!(
            budget.peak(),
            1 << 19,
            "a 256 KiB growth, then one the Engine failed to make, through MemoryBudget::peak \
             must report the 512 KiB the watermark reached"
        );
    }

    // @behavior S-062
    #[test]
    fn a_refused_growth_leaves_the_peak_alone() {
        let budget = MemoryBudget::new(Some(1 << 20));
        budget.arm(2 << 20);
        assert_charged(&budget, (2 << 20) + (1 << 19), "a 512 KiB growth");
        assert_refused(&budget, 4 << 20, "a 2 MiB growth under a 1 MiB limit");
        assert_eq!(
            budget.peak(),
            1 << 19,
            "a refused 2 MiB growth through MemoryBudget::peak must leave 512 KiB reported"
        );
    }

    // @behavior S-117
    #[test]
    fn each_arming_resets_the_peak() {
        let budget = MemoryBudget::new(Some(1 << 20));
        budget.arm(2 << 20);
        assert_charged(&budget, (2 << 20) + (1 << 19), "a 512 KiB growth");
        budget.arm(3 << 20);
        assert_eq!(
            budget.peak(),
            0,
            "a second arm through MemoryBudget::peak must report 0"
        );
    }

    // @behavior S-118
    #[test]
    fn a_budget_without_a_limit_still_reports_its_peak() {
        let budget = MemoryBudget::new(None);
        budget.arm(1 << 20);
        assert_charged(&budget, 5 << 20, "a 4 MiB growth with no limit");
        assert_eq!(
            budget.peak(),
            4 << 20,
            "a 4 MiB growth with no limit through MemoryBudget::peak must report 4 MiB"
        );
    }
}
