//! The memory one Invocation may take, as Linux's memory cgroup counts a
//! task's pages and the memory the kernel holds for it, socket buffers among
//! them, in one `memory.current` against `memory.max`.

use std::sync::{Arc, Mutex, PoisonError};

use crate::Errno;

/// What one Invocation has taken against its `memory_limit`: the guest's
/// memory grown past where the run began, and what the host holds on its
/// behalf between calls. `None` bounds neither, while both are still counted
/// so the peak is reported either way.
pub struct MemoryBudget {
    limit: Option<usize>,
    charged: Mutex<Charged>,
}

#[derive(Default)]
struct Charged {
    /// Whether the guest's growth is charged; it is not before the run
    /// begins, as the image's own allocation must pass.
    armed: bool,
    baseline: usize,
    grown: usize,
    held: usize,
    peak: usize,
}

impl Charged {
    fn total(&self) -> usize {
        self.grown.saturating_add(self.held)
    }
}

impl MemoryBudget {
    pub fn new(limit: Option<usize>) -> Arc<Self> {
        Arc::new(Self {
            limit,
            charged: Mutex::default(),
        })
    }

    /// Charge the guest's memory past `baseline` from now on, as the run
    /// begins, so the image's allocation and what earlier runs left behind
    /// take none of it.
    pub fn arm(&self, baseline: usize) {
        let mut charged = self.charged();
        charged.armed = true;
        charged.baseline = baseline;
        charged.grown = 0;
        charged.peak = charged.held;
    }

    /// Stop charging the guest's growth once the run returns, so reading its
    /// answer back costs the script nothing.
    pub fn disarm(&self) {
        self.charged().armed = false;
    }

    /// The most the run held at once. A charge the budget refused never
    /// raises it, so it never passes `memory_limit`.
    pub fn peak(&self) -> usize {
        self.charged().peak
    }

    /// Charge the guest's memory growing to `desired` bytes, refused where it
    /// and what the host holds would pass the limit. What it answers gives the
    /// charge back should the growth then fail.
    pub fn charge_growth(&self, desired: usize) -> Result<GrowthCharge, MemoryLimitExceeded> {
        let mut charged = self.charged();
        let before = GrowthCharge {
            grown: charged.grown,
        };
        if !charged.armed {
            return Ok(before);
        }
        let grown = desired.saturating_sub(charged.baseline);
        let total = grown.saturating_add(charged.held);
        if let Some(limit) = self.limit.filter(|&limit| total > limit) {
            return Err(MemoryLimitExceeded::new(desired, limit));
        }
        charged.grown = grown;
        charged.peak = charged.peak.max(total);
        Ok(before)
    }

    /// Give back `charge`, a growth the Engine failed to make once the budget
    /// allowed it, so what the run holds counts no memory it never had, as
    /// `page_counter_cancel` gives a charge back. The peak keeps it, as the
    /// watermark the charge raised stays.
    pub fn cancel_growth(&self, charge: GrowthCharge) {
        self.charged().grown = charge.grown;
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

/// What the guest's growth stood at before one charge, which a growth the
/// Engine fails to make gives back through `MemoryBudget::cancel_growth`.
#[must_use]
pub struct GrowthCharge {
    grown: usize,
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
    //! The arithmetic of one budget, so a regression that breaks the baseline
    //! (dropping its subtraction, or letting `arm` carry stale state across
    //! invocations) or counts the guest and the host apart is caught without
    //! an Engine.
    use super::MemoryBudget;
    use crate::Errno;

    fn assert_charged(budget: &MemoryBudget, desired: usize, what: &str) {
        assert!(
            budget.charge_growth(desired).is_ok(),
            "{what} through MemoryBudget::charge_growth must be allowed"
        );
    }

    fn assert_refused(budget: &MemoryBudget, desired: usize, what: &str) {
        assert!(
            budget.charge_growth(desired).is_err(),
            "{what} through MemoryBudget::charge_growth must be refused"
        );
    }

    // @behavior S-113
    #[test]
    fn a_growth_before_the_budget_is_armed_is_not_bounded() {
        let budget = MemoryBudget::new(Some(1 << 20));
        assert_charged(&budget, 100 << 20, "a 100 MiB growth before arm");
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
        assert_charged(&budget, (2 << 20) + (1 << 19), "a 512 KiB growth");
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
