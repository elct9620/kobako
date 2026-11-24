//! The memory one Invocation may take, as Linux's memory cgroup counts a
//! task's pages and the memory the kernel holds for it, socket buffers among
//! them, in one `memory.current` against `memory.max`.

use std::sync::{Arc, Mutex, PoisonError};

use crate::Errno;

/// What one Invocation has taken against its `memory_limit`: the guest's
/// memories, tables, and heaps grown past what its Guest Binary declares,
/// and what the host holds on its behalf between calls. `None` bounds
/// neither, while both are still counted so the peak is reported either way.
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
    held: usize,
    peak: usize,
}

impl Charged {
    fn total(&self) -> usize {
        self.grown
            .saturating_sub(self.declared)
            .saturating_add(self.held)
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
        charged.peak = charged.held;
    }

    /// Stop charging the guest's growth once the run returns, so reading its
    /// answer back costs the script nothing.
    pub fn disarm(&self) {
        self.charged().armed = false;
    }

    /// The most the run held at once. A charge the budget refused never
    /// raises it; a forced one does, past `memory_limit`, as `memory.peak`
    /// records usage a forced charge took over `memory.max`.
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

    /// Charge `bytes` the host holds for the guest, refused as ENOMEM where
    /// they would pass the limit, as `try_charge_memcg` refuses a charge past
    /// `memory.max`.
    pub(crate) fn try_charge(&self, bytes: usize) -> Result<(), Errno> {
        let mut charged = self.charged();
        let total = charged
            .total()
            .checked_add(bytes)
            .filter(|&total| self.limit.is_none_or(|limit| total <= limit))
            .ok_or(Errno::OutOfMemory)?;
        charged.held += bytes;
        charged.peak = charged.peak.max(total);
        Ok(())
    }

    /// Charge `bytes` the host must hold for the guest to move on, past the
    /// limit if need be, as `try_charge_memcg` force-charges an allocation
    /// that frees memory soon. While the total stays past the limit, every
    /// other charge and the guest's growth are refused.
    fn force_charge(&self, bytes: usize) {
        let mut charged = self.charged();
        charged.held = charged.held.saturating_add(bytes);
        charged.peak = charged.peak.max(charged.total());
    }

    /// Whether `bytes` more would stay within the limit.
    fn can_charge(&self, bytes: usize) -> bool {
        let total = self.charged().total();
        self.limit
            .is_none_or(|limit| total.checked_add(bytes).is_some_and(|total| total <= limit))
    }

    /// Whether what it holds reached the limit, so no charge can grow.
    fn is_over_limit(&self) -> bool {
        let total = self.charged().total();
        self.limit.is_some_and(|limit| total >= limit)
    }

    /// `bytes` charged earlier are held no more, as `memcg_uncharge` gives
    /// them back.
    fn uncharge(&self, bytes: usize) {
        let mut charged = self.charged();
        charged.held = charged.held.saturating_sub(bytes);
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

/// Memory the host holds for a guest, charged to its budget as it is taken
/// and given back as it is let go, as `obj_cgroup_charge` and
/// `obj_cgroup_uncharge` count what the kernel holds for a cgroup by the byte:
/// a socket's queues, a listing, or a part of a request the guest builds.
pub struct MemoryCharge {
    budget: Arc<MemoryBudget>,
    bytes: usize,
}

impl MemoryCharge {
    /// Charge `bytes` to `budget`, refused past its limit as `ENOMEM`.
    pub fn new(budget: &Arc<MemoryBudget>, bytes: usize) -> Result<Self, Errno> {
        budget.try_charge(bytes)?;
        Ok(Self {
            budget: budget.clone(),
            bytes,
        })
    }

    /// A charge to `budget` that holds nothing yet.
    pub fn empty(budget: &Arc<MemoryBudget>) -> Self {
        Self {
            budget: budget.clone(),
            bytes: 0,
        }
    }

    /// The bytes it holds.
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    /// Hold `bytes` in place of what it held, charging only the growth and
    /// giving back the rest; refused past the limit as `ENOMEM`, keeping what
    /// it held.
    pub fn resize(&mut self, bytes: usize) -> Result<(), Errno> {
        match bytes.checked_sub(self.bytes) {
            Some(growth) => self.budget.try_charge(growth)?,
            None => self.budget.uncharge(self.bytes - bytes),
        }
        self.bytes = bytes;
        Ok(())
    }

    /// Whether its budget reached the limit, so no charge can grow.
    pub fn is_over_limit(&self) -> bool {
        self.budget.is_over_limit()
    }

    /// Whether holding `bytes` in place of what it holds would stay within
    /// the limit.
    pub fn can_resize(&self, bytes: usize) -> bool {
        self.budget.can_charge(bytes.saturating_sub(self.bytes))
    }

    /// Hold `bytes` in place of what it held, charging the growth even past
    /// the limit, as `sk_forced_mem_schedule` charges what a socket must hold
    /// to move on.
    pub fn force_resize(&mut self, bytes: usize) {
        match bytes.checked_sub(self.bytes) {
            Some(growth) => self.budget.force_charge(growth),
            None => self.budget.uncharge(self.bytes - bytes),
        }
        self.bytes = bytes;
    }
}

impl Drop for MemoryCharge {
    fn drop(&mut self) {
        self.budget.uncharge(self.bytes);
    }
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
    //! invocations) or counts the guest and the host apart is caught without
    //! an Engine.
    use super::MemoryBudget;
    use crate::Errno;

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

    // @behavior S-209
    #[test]
    fn a_growth_past_what_the_host_left_of_the_limit_is_refused() {
        let budget = MemoryBudget::new(Some(1 << 20));
        budget.arm(2 << 20);
        budget
            .try_charge(3 << 18)
            .expect("768 KiB of socket buffers under a 1 MiB limit must be charged");
        assert_refused(
            &budget,
            (2 << 20) + (1 << 19),
            "a 512 KiB growth beside 768 KiB the host holds under a 1 MiB limit",
        );
    }

    // @behavior S-209
    #[test]
    fn a_host_charge_past_what_the_guest_left_of_the_limit_is_refused() {
        let budget = MemoryBudget::new(Some(1 << 20));
        budget.arm(2 << 20);
        assert_charged(&budget, (2 << 20) + (3 << 18), "a 768 KiB growth");
        assert_eq!(
            budget.try_charge(1 << 19),
            Err(Errno::OutOfMemory),
            "512 KiB of host buffers beside a 768 KiB growth under a 1 MiB limit must answer ENOMEM"
        );
    }

    // @behavior S-062
    #[test]
    fn a_forced_charge_passes_the_limit_until_it_is_given_back() {
        let budget = MemoryBudget::new(Some(1 << 20));
        budget.arm(2 << 20);
        assert_charged(&budget, (2 << 20) + (3 << 18), "a 768 KiB growth");
        let budget = std::sync::Arc::new(budget);
        let mut charge = super::MemoryCharge::empty(&budget);
        charge.force_resize(1 << 19);
        let while_past = (
            budget.peak(),
            budget.try_charge(1),
            budget.charge_growth(1).is_err(),
        );
        charge.force_resize(0);
        assert_eq!(
            (while_past, budget.try_charge(1 << 17)),
            (
                ((3 << 18) + (1 << 19), Err(Errno::OutOfMemory), true),
                Ok(())
            ),
            "512 KiB forced beside a 768 KiB growth under a 1 MiB limit must raise the peak to \
             1.25 MiB and refuse every other charge and growth, until it is given back"
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
        assert_charged(&budget, 1 << 18, "another 256 KiB growth");
        budget
            .try_charge(1 << 17)
            .expect("128 KiB of host buffers must be charged");
        budget.uncharge(1 << 17);
        assert_eq!(
            budget.peak(),
            (1 << 19) + (1 << 17),
            "a 512 KiB growth beside 128 KiB the host held for a while through \
             MemoryBudget::peak must report 640 KiB"
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
