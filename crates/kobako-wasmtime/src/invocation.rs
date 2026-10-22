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
    on_dispatch: Option<Arc<dyn DispatchHandler>>,
    deadline: Option<Instant>,
    limiter: MemoryLimiter,
    wall_entry: Option<Instant>,
    wall_time: Duration,
    reentry_trap: Option<wasmtime::Error>,
    exports: Option<Exports>,
    open_calls: u32,
}

impl Invocation {
    pub(crate) fn new(memory_limit: Option<usize>, wasi: Wasi) -> Self {
        Self {
            wasi,
            on_dispatch: None,
            deadline: None,
            limiter: MemoryLimiter::new(memory_limit),
            wall_entry: None,
            wall_time: Duration::ZERO,
            reentry_trap: None,
            exports: None,
            open_calls: 0,
        }
    }

    pub(crate) fn set_exports(&mut self, exports: Exports) {
        self.exports = Some(exports);
    }

    /// `None` until the instance exists, so a host import reached before
    /// then finds no guest to call back into.
    pub(crate) fn exports(&self) -> Option<Exports> {
        self.exports.clone()
    }

    /// The first trap wins: the upcall import ends the invocation with it,
    /// so the trap keeps its own kind.
    pub(crate) fn record_reentry_trap(&mut self, trap: wasmtime::Error) {
        self.reentry_trap.get_or_insert(trap);
    }

    /// Once a callback into the guest has trapped, the guest is past
    /// resuming.
    pub(crate) fn reentry_trapped(&self) -> bool {
        self.reentry_trap.is_some()
    }

    pub(crate) fn take_reentry_trap(&mut self) -> Option<wasmtime::Error> {
        self.reentry_trap.take()
    }

    /// Count a Call the host is about to answer and name its frame, as the
    /// guest counts the Calls it has open.
    pub(crate) fn open_call(&mut self) -> u32 {
        self.open_calls += 1;
        self.open_calls - 1
    }

    pub(crate) fn close_call(&mut self) {
        self.open_calls -= 1;
    }

    pub(crate) fn bind_on_dispatch(&mut self, handler: Arc<dyn DispatchHandler>) {
        self.on_dispatch = Some(handler);
    }

    /// A clone, so the borrow on the `Caller` is released and the
    /// dispatcher can re-borrow it to write the Reply.
    pub(crate) fn on_dispatch(&self) -> Option<Arc<dyn DispatchHandler>> {
        self.on_dispatch.clone()
    }

    /// The trap ending an invocation its host gave up on.
    pub(crate) fn abandonment(&self) -> Option<wasmtime::Error> {
        self.on_dispatch
            .as_ref()
            .is_some_and(|handler| handler.abandoned())
            .then(|| wasmtime::Error::msg("the host abandoned the invocation"))
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
        self.limiter.activate(baseline);
    }

    /// Disarmed once the export returns, so post-run host bookkeeping such
    /// as reading the answer back is not charged to the script.
    pub(crate) fn disarm_memory_cap(&mut self) {
        self.limiter.deactivate();
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
        self.limiter.peak()
    }
}
