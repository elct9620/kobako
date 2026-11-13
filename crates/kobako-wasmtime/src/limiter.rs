//! Hands wasmtime's growth hook to the runtime's per-invocation memory budget.

use std::sync::Arc;

use kobako_runtime::memcg::{GrowthCharge, MemoryBudget};
use wasmtime::ResourceLimiter;

/// wasmtime asks this as `memory.grow` runs; an `Err` becomes the trap
/// surfaced to the host as a guest invocation failure. A growth it allowed
/// may still fail, past the memory's maximum or for want of host memory, and
/// its charge is then given back.
pub(crate) struct MemoryLimiter {
    budget: Arc<MemoryBudget>,
    pending: Option<GrowthCharge>,
}

impl MemoryLimiter {
    pub(crate) fn new(budget: Arc<MemoryBudget>) -> Self {
        Self {
            budget,
            pending: None,
        }
    }

    /// The budget it charges.
    pub(crate) fn budget(&self) -> &MemoryBudget {
        &self.budget
    }
}

impl ResourceLimiter for MemoryLimiter {
    fn memory_growing(
        &mut self,
        _current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        let charge = self
            .budget
            .charge_growth(desired)
            .map_err(wasmtime::Error::new)?;
        self.pending = Some(charge);
        Ok(true)
    }

    fn memory_grow_failed(&mut self, _error: wasmtime::Error) -> wasmtime::Result<()> {
        if let Some(charge) = self.pending.take() {
            self.budget.cancel_growth(charge);
        }
        Ok(())
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

#[cfg(test)]
mod tests {
    use wasmtime::{Engine, Memory, MemoryType, Store};

    use super::*;

    const PAGE: usize = 64 * 1024;

    // wasmtime asks the limiter before it checks the memory's own maximum, so
    // only a growth it fails after the budget allowed it shows the charge
    // given back.
    // @behavior S-211
    #[test]
    fn a_growth_the_engine_fails_to_make_costs_the_budget_nothing() {
        let budget = MemoryBudget::new(Some(4 * PAGE));
        budget.arm(PAGE);
        let mut store = Store::new(&Engine::default(), MemoryLimiter::new(budget.clone()));
        store.limiter(|limiter| limiter);
        let memory = Memory::new(&mut store, MemoryType::new(1, Some(2)))
            .expect("a memory of one page within its maximum must be made");
        let failed = memory.grow(&mut store, 2).is_err();
        let regrown = memory.grow(&mut store, 1).is_ok();
        assert_eq!(
            (failed, regrown, budget.peak()),
            (true, true, PAGE),
            "a growth past a memory's maximum that the budget allowed must fail and give its \
             charge back, leaving the budget a growth within the maximum and the peak it reached"
        );
    }
}
