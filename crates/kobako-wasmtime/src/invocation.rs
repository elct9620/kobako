//! Per-invocation host state — one `Invocation` per OS thread for the
//! lifetime of one `Driver` invoke call.
//!
//! Owned as the data of each per-invocation `wasmtime::Store`
//! and threaded through every host import —
//! the `__kobako_dispatch` dispatcher reads the bound dispatch handler,
//! while `Driver::invoke` installs the invocation's WASI
//! context + pipes (via `frames::install_wasi_frames`) before the guest
//! export call.
//!
//! The slot also carries the per-invocation wall-clock deadline and the
//! memory cap `MemoryLimiter`, both read from the wasmtime
//! `epoch_deadline_callback` / `ResourceLimiter` callbacks installed in
//! `Driver::new_store`.

use std::sync::Arc;
use std::time::{Duration, Instant};

use wasmtime_wasi::p1::WasiP1Ctx;
use wasmtime_wasi::p2::pipe::MemoryOutputPipe;

use kobako_runtime::dispatch::DispatchHandler;

use crate::limiter::MemoryLimiter;

/// Per-invocation host state — the data half of the Single-Invocation
/// Slot. Threaded through every host import callback.
///
/// All field access is mediated by methods on this type — the WASI ctx
/// is rebuilt fresh before each invocation via
/// `Invocation::install_wasi`, the dispatch handler is set once via
/// `Invocation::bind_on_dispatch`, and captured stdout/stderr bytes
/// are read after the invocation via `Invocation::stdout_bytes` /
/// `Invocation::stderr_bytes`. The fields are private so the mutation
/// surface stays narrow.
pub(crate) struct Invocation {
    wasi: Option<WasiP1Ctx>,
    stdout_pipe: Option<MemoryOutputPipe>,
    stderr_pipe: Option<MemoryOutputPipe>,
    on_dispatch: Option<Arc<dyn DispatchHandler>>,
    deadline: Option<Instant>,
    limiter: MemoryLimiter,
    wall_entry: Option<Instant>,
    wall_time: Duration,
    reentry_trap: Option<wasmtime::Error>,
}

impl Invocation {
    pub(crate) fn new(memory_limit: Option<usize>) -> Self {
        Self {
            wasi: None,
            stdout_pipe: None,
            stderr_pipe: None,
            on_dispatch: None,
            deadline: None,
            limiter: MemoryLimiter::new(memory_limit),
            wall_entry: None,
            wall_time: Duration::ZERO,
            reentry_trap: None,
        }
    }

    /// The first trap wins: the dispatch import ends the invocation with it,
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

    pub(crate) fn install_wasi(
        &mut self,
        wasi: WasiP1Ctx,
        stdout: MemoryOutputPipe,
        stderr: MemoryOutputPipe,
    ) {
        self.wasi = Some(wasi);
        self.stdout_pipe = Some(stdout);
        self.stderr_pipe = Some(stderr);
    }

    pub(crate) fn bind_on_dispatch(&mut self, handler: Arc<dyn DispatchHandler>) {
        self.on_dispatch = Some(handler);
    }

    pub(crate) fn stdout_bytes(&self) -> Vec<u8> {
        self.stdout_pipe
            .as_ref()
            .map(|p| p.contents().to_vec())
            .unwrap_or_default()
    }

    pub(crate) fn stderr_bytes(&self) -> Vec<u8> {
        self.stderr_pipe
            .as_ref()
            .map(|p| p.contents().to_vec())
            .unwrap_or_default()
    }

    /// A clone, so the borrow on the `Caller` is released and the
    /// dispatcher can re-borrow it to write the response.
    pub(crate) fn on_dispatch(&self) -> Option<Arc<dyn DispatchHandler>> {
        self.on_dispatch.clone()
    }

    pub(crate) fn wasi_mut(&mut self) -> &mut WasiP1Ctx {
        self.wasi.as_mut().expect(
            "WASI context not initialised — the driver must install frames before any WASI use",
        )
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
    /// as fetching the OUTCOME_BUFFER is not charged to the script.
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
