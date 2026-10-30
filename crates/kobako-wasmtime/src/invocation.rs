//! Per-invocation host state — one `Invocation` per OS thread for the
//! lifetime of one `Driver` invoke call.
//!
//! Owned as the data of each per-invocation `wasmtime::Store`
//! and threaded through every host import —
//! the `__kobako_upcall` dispatcher reads the bound dispatch handler,
//! and the WASI imports read the invocation's `Wasi` context, which the
//! Store is created with.
//!
//! The slot also carries the per-invocation wall-clock deadline and the
//! memory cap `MemoryLimiter`, both read from the wasmtime
//! `epoch_deadline_callback` / `ResourceLimiter` callbacks installed in
//! `Driver::new_store`.

use std::sync::Arc;
use std::time::{Duration, Instant};

use kobako_runtime::dispatch::DispatchHandler;
use kobako_runtime::endpoint::{Calls, MemoryCap, ABANDONED};

use crate::exports::Exports;
use crate::limiter::MemoryLimiter;
use crate::wasi::Wasi;

/// Per-invocation host state, owned by the invocation's own Store.
/// Threaded through every host import callback.
///
/// All field access is mediated by methods on this type — the `Wasi`
/// context arrives with the invocation, the dispatch handler is set once
/// via `Invocation::bind_on_dispatch`, and the captures are read back
/// through `Invocation::wasi` after the run. The fields are private so
/// the mutation surface stays narrow.
pub(crate) struct Invocation {
    wasi: Wasi,
    calls: Calls<wasmtime::Error>,
    deadline: Option<Instant>,
    limiter: MemoryLimiter,
    wall_entry: Option<Instant>,
    wall_time: Duration,
    exports: Option<Exports>,
    hostcall_fuel: usize,
}

impl Invocation {
    pub(crate) fn new(memory_limit: Option<usize>, wasi: Wasi) -> Self {
        Self {
            wasi,
            calls: Calls::default(),
            deadline: None,
            limiter: MemoryLimiter(MemoryCap::new(memory_limit)),
            wall_entry: None,
            wall_time: Duration::ZERO,
            exports: None,
            hostcall_fuel: 0,
        }
    }

    /// How much of the guest's memory the call under way may have the host
    /// copy or walk, as wasmtime hands each host call.
    pub(crate) fn hostcall_fuel(&self) -> usize {
        self.hostcall_fuel
    }

    pub(crate) fn set_hostcall_fuel(&mut self, fuel: usize) {
        self.hostcall_fuel = fuel;
    }

    pub(crate) fn set_exports(&mut self, exports: Exports) {
        self.exports = Some(exports);
    }

    /// `None` until the instance exists, so a host import reached before
    /// then finds no guest to call back into.
    pub(crate) fn exports(&self) -> Option<Exports> {
        self.exports.clone()
    }

    /// The state of the Calls open on this instance.
    pub(crate) fn calls_mut(&mut self) -> &mut Calls<wasmtime::Error> {
        &mut self.calls
    }

    pub(crate) fn bind_on_dispatch(&mut self, handler: Arc<dyn DispatchHandler>) {
        self.calls.bind(handler);
    }

    /// The trap ending an invocation its host gave up on.
    pub(crate) fn abandonment(&self) -> Option<wasmtime::Error> {
        self.calls
            .abandoned()
            .then(|| wasmtime::Error::msg(ABANDONED))
    }

    pub(crate) fn wasi(&self) -> &Wasi {
        &self.wasi
    }

    pub(crate) fn wasi_mut(&mut self) -> &mut Wasi {
        &mut self.wasi
    }

    pub(crate) fn set_deadline(&mut self, deadline: Option<Instant>) {
        self.deadline = deadline;
    }

    pub(crate) fn deadline(&self) -> Option<Instant> {
        self.deadline
    }

    pub(crate) fn limiter_mut(&mut self) -> &mut MemoryLimiter {
        &mut self.limiter
    }

    /// Only the `memory.grow` delta past `baseline` is charged, so the
    /// image's initial allocation and the high-water mark of earlier
    /// invocations do not consume the budget.
    pub(crate) fn arm_memory_cap(&mut self, baseline: usize) {
        self.limiter.0.activate(baseline);
    }

    /// Disarmed once the export returns, so post-run host bookkeeping such
    /// as reading the answer back is not charged to the script.
    pub(crate) fn disarm_memory_cap(&mut self) {
        self.limiter.0.deactivate();
    }

    /// Stamped right before the guest export call, so the bracket matches
    /// the `timeout` accounting and excludes post-run host bookkeeping.
    pub(crate) fn start_wall_clock(&mut self) {
        self.wall_entry = Some(Instant::now());
    }

    pub(crate) fn stop_wall_clock(&mut self) {
        if let Some(entry) = self.wall_entry.take() {
            self.wall_time = entry.elapsed();
        }
    }

    pub(crate) fn wall_time(&self) -> Duration {
        self.wall_time
    }

    pub(crate) fn memory_peak(&self) -> usize {
        self.limiter.0.peak()
    }
}
