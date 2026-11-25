//! Hands wasmtime's growth hook to the runtime's per-invocation memory budget.

use std::sync::Arc;

use kobako_runtime::memcg::{GrowthCharge, MemoryBudget, MemoryLimitExceeded};
use wasmtime::{Module, ResourceLimiter};

/// What one wasm page of a memory takes; the Engine enables no custom page
/// size.
const PAGE: usize = 64 * 1024;

/// What one table entry takes, a pointer in wasmtime.
const TABLE_ENTRY: usize = size_of::<usize>();

/// The bytes the memories and tables `module` defines start with, which
/// wasmtime reports as their count and the largest initial size, so a module
/// whose sizes differ is granted the largest for each.
pub(crate) fn declared(module: &Module) -> usize {
    let required = module.resources_required();
    let memory = to_usize(required.max_initial_memory_size).saturating_mul(PAGE);
    let table = to_usize(required.max_initial_table_size).saturating_mul(TABLE_ENTRY);
    (required.num_memories as usize)
        .saturating_mul(memory)
        .saturating_add((required.num_tables as usize).saturating_mul(table))
}

fn to_usize(size: Option<u64>) -> usize {
    size.map_or(0, |size| usize::try_from(size).unwrap_or(usize::MAX))
}

/// wasmtime asks this as a memory, a table, or the GC heap is created or
/// grows; a growth past the budget fails and is kept as the refusal that a
/// run failing after it is attributed to. A growth it allowed may still
/// fail, past the memory's maximum or for want of host memory, and its
/// charge is then given back.
pub(crate) struct MemoryLimiter {
    budget: Arc<MemoryBudget>,
    pending: Option<GrowthCharge>,
    refused: Option<MemoryLimitExceeded>,
}

impl MemoryLimiter {
    pub(crate) fn new(budget: Arc<MemoryBudget>) -> Self {
        Self {
            budget,
            pending: None,
            refused: None,
        }
    }

    /// The budget it charges.
    pub(crate) fn budget(&self) -> &MemoryBudget {
        &self.budget
    }

    /// The last growth the budget refused. The guest only sees the growth
    /// fail, so this is what tells a run that then fails apart as the
    /// budget's.
    pub(crate) fn refused(&self) -> Option<&MemoryLimitExceeded> {
        self.refused.as_ref()
    }

    /// A growth the budget refuses fails, as `memory.grow` answers -1, rather
    /// than ending the run, so the guest can collect what it no longer holds
    /// and try again, as mruby does once its allocator answers none.
    fn charge(&mut self, bytes: usize) -> wasmtime::Result<bool> {
        match self.budget.charge_growth(bytes) {
            Ok(charge) => {
                self.pending = Some(charge);
                Ok(true)
            }
            Err(refusal) => {
                self.refused = Some(refusal);
                Ok(false)
            }
        }
    }

    fn give_back(&mut self) {
        if let Some(charge) = self.pending.take() {
            self.budget.cancel_growth(charge);
        }
    }
}

impl ResourceLimiter for MemoryLimiter {
    fn memory_growing(
        &mut self,
        current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        self.charge(desired.saturating_sub(current))
    }

    fn memory_grow_failed(&mut self, _error: wasmtime::Error) -> wasmtime::Result<()> {
        self.give_back();
        Ok(())
    }

    fn table_growing(
        &mut self,
        current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        self.charge(desired.saturating_sub(current).saturating_mul(TABLE_ENTRY))
    }

    fn table_grow_failed(&mut self, _error: wasmtime::Error) -> wasmtime::Result<()> {
        self.give_back();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use wasmtime::{Memory, MemoryType, Store};

    use super::*;

    const PAGE: usize = 64 * 1024;

    // wasmtime asks the limiter before it checks the memory's own maximum, so
    // only a growth it fails after the budget allowed it shows the charge
    // given back.
    // @behavior S-211
    #[test]
    fn a_growth_the_engine_fails_to_make_costs_the_budget_nothing() {
        let budget = MemoryBudget::new(Some(3 * PAGE));
        budget.arm(PAGE);
        let engine = crate::cache::load_engine().expect("the shared Engine must be built");
        let mut store = Store::new(engine, MemoryLimiter::new(budget.clone()));
        store.limiter(|limiter| limiter);
        let memory = Memory::new(&mut store, MemoryType::new(1, Some(2)))
            .expect("a memory of one page within its maximum must be made");
        let failed = memory.grow(&mut store, 2).is_err();
        let peak = budget.peak();
        let regrown = budget.charge_growth(2 * PAGE).is_ok();
        assert_eq!(
            (failed, peak, regrown),
            (true, 2 * PAGE, true),
            "a growth past a memory's maximum that the budget allowed must fail, give the \
             budget back for the next growth, and stay in the peak reported"
        );
    }
}
