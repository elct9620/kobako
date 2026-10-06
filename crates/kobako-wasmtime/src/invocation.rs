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
//! The slot also carries the per-invocation wall-clock deadline
//! and the per-invocation linear-memory
//! delta cap `MemoryLimiter`. Both are
//! read from the wasmtime `epoch_deadline_callback` / `ResourceLimiter`
//! callbacks installed in `Driver::new_store`. The
//! memory cap measures only the `memory.grow` delta past the linear-
//! memory size captured at invocation entry — the image's initial
//! allocation is outside the budget.

use std::sync::Arc;
use std::time::{Duration, Instant};

use wasmtime::ResourceLimiter;
use wasmtime_wasi::p1::WasiP1Ctx;
use wasmtime_wasi::p2::pipe::MemoryOutputPipe;

use kobako_runtime::dispatch::DispatchHandler;

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

/// Resource limiter that enforces the per-invocation `memory_limit`
/// cap.
///
/// `max_memory` is the byte cap on per-invocation growth (`None` disables
/// the cap). `baseline` is the linear-memory size captured at invocation
/// entry by `MemoryLimiter::activate`; the limiter charges only the
/// `memory.grow` delta past `baseline` against `max_memory`, so the
/// mruby image's initial allocation and any high-water mark left by
/// prior invocations on the same Sandbox do not consume the budget.
/// `cap_active` gates whether the cap is enforced — wasmtime's
/// `ResourceLimiter` also fires for the module's declared initial
/// allocation at instantiation time, but the cap stays dormant until
/// `MemoryLimiter::activate` flips the flag for one `Driver` invoke
/// call. When `cap_active` is `false`, the limiter always allows
/// growth.
///
/// When `memory.grow` would push the per-invocation delta past
/// `max_memory`, the limiter returns `MemoryLimitTrap` from
/// `memory_growing`; wasmtime turns that into the trap surfaced to the
/// host as a guest invocation failure.
#[derive(Debug, Clone, Copy)]
pub(crate) struct MemoryLimiter {
    max_memory: Option<usize>,
    baseline: usize,
    cap_active: bool,
    peak: usize,
}

impl MemoryLimiter {
    fn new(max_memory: Option<usize>) -> Self {
        Self {
            max_memory,
            baseline: 0,
            cap_active: false,
            peak: 0,
        }
    }

    /// The cap stays dormant until armed, because the module's declared
    /// initial memory is allocated during instantiation and must pass.
    fn activate(&mut self, baseline: usize) {
        self.baseline = baseline;
        self.cap_active = true;
        self.peak = 0;
    }

    fn deactivate(&mut self) {
        self.cap_active = false;
    }

    /// A grow the cap rejects never updates the peak, so the reported value
    /// never exceeds `memory_limit`.
    pub(crate) fn peak(&self) -> usize {
        self.peak
    }
}

impl ResourceLimiter for MemoryLimiter {
    fn memory_growing(
        &mut self,
        _current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        if !self.cap_active {
            return Ok(true);
        }
        let delta = desired.saturating_sub(self.baseline);
        if let Some(limit) = self.max_memory {
            if delta > limit {
                return Err(wasmtime::Error::new(MemoryLimitTrap { desired, limit }));
            }
        }
        if delta > self.peak {
            self.peak = delta;
        }
        Ok(true)
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

/// Marker error returned from `MemoryLimiter::memory_growing` when the
/// per-invocation memory cap is exceeded. Downcast from the wasmtime
/// trap error to classify the failure as a memory-limit `Trap`. Callers
/// use the `Display` impl below — no field is read directly — so the
/// inner state stays private.
#[derive(Debug)]
pub(crate) struct MemoryLimitTrap {
    desired: usize,
    limit: usize,
}

impl MemoryLimitTrap {
    #[cfg(test)]
    pub(crate) fn new(desired: usize, limit: usize) -> Self {
        Self { desired, limit }
    }
}

impl std::fmt::Display for MemoryLimitTrap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "memory usage exceeded memory_limit: \
             requested={} bytes, limit={} bytes",
            self.desired, self.limit
        )
    }
}

impl std::error::Error for MemoryLimitTrap {}

/// Marker error returned from the epoch-deadline callback when the
/// wall-clock deadline is exceeded. Downcast from the wasmtime trap
/// error to classify the failure as a timeout `Trap`.
#[derive(Debug)]
pub(crate) struct TimeoutTrap;

impl std::fmt::Display for TimeoutTrap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "wall-clock deadline exceeded")
    }
}

impl std::error::Error for TimeoutTrap {}

#[cfg(test)]
mod tests {
    //! Unit tests for `MemoryLimiter` — the per-invocation memory
    //! delta cap. The Ruby-facing E2E suite exercises the full path
    //! through wasmtime; these tests pin the pure delta arithmetic so
    //! a regression that breaks the baseline accounting (e.g. dropping
    //! the `baseline` subtraction, or letting `activate` carry stale
    //! state across invocations) is caught without spinning up a
    //! Store.
    use super::{MemoryLimitTrap, MemoryLimiter};
    use wasmtime::ResourceLimiter;

    fn assert_growing(limiter: &mut MemoryLimiter, desired: usize) {
        assert!(
            limiter.memory_growing(0, desired, None).unwrap(),
            "expected memory_growing({desired}) to allow growth"
        );
    }

    fn assert_trapping(limiter: &mut MemoryLimiter, desired: usize) {
        let err = limiter
            .memory_growing(0, desired, None)
            .expect_err("expected memory_growing to trap");
        assert!(
            err.downcast_ref::<MemoryLimitTrap>().is_some(),
            "expected MemoryLimitTrap, got {err:?}"
        );
    }

    // @behavior S-113
    #[test]
    fn dormant_limiter_allows_any_growth() {
        let mut limiter = MemoryLimiter::new(Some(1 << 20));
        // Without `activate`, the cap is dormant — the module's
        // declared initial allocation must pass through unconditionally.
        assert_growing(&mut limiter, 100 << 20);
    }

    // @behavior S-114
    #[test]
    fn delta_below_cap_passes_after_activate() {
        let mut limiter = MemoryLimiter::new(Some(1 << 20));
        limiter.activate(2 << 20);
        // baseline=2 MiB, desired=2.5 MiB → delta=0.5 MiB ≤ 1 MiB cap.
        assert_growing(&mut limiter, (2 << 20) + (1 << 19));
    }

    // @behavior S-121
    #[test]
    fn delta_past_cap_traps_with_memory_limit_trap() {
        let mut limiter = MemoryLimiter::new(Some(1 << 20));
        limiter.activate(2 << 20);
        // baseline=2 MiB, desired=4 MiB → delta=2 MiB > 1 MiB cap.
        assert_trapping(&mut limiter, 4 << 20);
    }

    // @behavior S-008
    #[test]
    fn activate_resets_baseline_on_each_invocation() {
        let mut limiter = MemoryLimiter::new(Some(1 << 20));
        limiter.activate(2 << 20);
        assert_growing(&mut limiter, (2 << 20) + (1 << 20));
        // Second invocation: linear memory has grown to 3 MiB. Re-arming
        // must re-anchor the baseline so the next 1 MiB of growth fits
        // the per-invocation budget rather than being charged against
        // the prior invocation's residue.
        limiter.activate(3 << 20);
        assert_growing(&mut limiter, (3 << 20) + (1 << 20));
    }

    // @behavior S-009
    #[test]
    fn disabled_cap_ignores_delta_size() {
        let mut limiter = MemoryLimiter::new(None);
        limiter.activate(0);
        assert_growing(&mut limiter, 100 << 20);
    }

    // @behavior S-116
    #[test]
    fn peak_starts_at_zero_before_any_grow() {
        let limiter = MemoryLimiter::new(Some(1 << 20));
        assert_eq!(limiter.peak(), 0);
    }

    // @behavior S-115
    #[test]
    fn peak_tracks_high_water_of_delta_past_baseline() {
        let mut limiter = MemoryLimiter::new(Some(1 << 20));
        limiter.activate(2 << 20);
        assert_growing(&mut limiter, (2 << 20) + (1 << 18)); // delta=256 KiB
        assert_growing(&mut limiter, (2 << 20) + (1 << 19)); // delta=512 KiB (new peak)
        assert_growing(&mut limiter, (2 << 20) + (1 << 17)); // delta=128 KiB (below peak)
        assert_eq!(limiter.peak(), 1 << 19);
    }

    // @behavior S-062
    #[test]
    fn trap_does_not_update_peak() {
        let mut limiter = MemoryLimiter::new(Some(1 << 20));
        limiter.activate(2 << 20);
        assert_growing(&mut limiter, (2 << 20) + (1 << 19)); // delta=512 KiB
        assert_trapping(&mut limiter, (2 << 20) + (2 << 20)); // would be 2 MiB > 1 MiB cap
                                                              // Peak reflects the last accepted grow, not the rejected desired.
        assert_eq!(limiter.peak(), 1 << 19);
    }

    // @behavior S-117
    #[test]
    fn activate_resets_peak_for_new_invocation() {
        let mut limiter = MemoryLimiter::new(Some(1 << 20));
        limiter.activate(2 << 20);
        assert_growing(&mut limiter, (2 << 20) + (1 << 19));
        assert_eq!(limiter.peak(), 1 << 19);
        limiter.activate(3 << 20);
        assert_eq!(limiter.peak(), 0);
    }

    // @behavior S-118
    #[test]
    fn disabled_cap_still_tracks_peak() {
        let mut limiter = MemoryLimiter::new(None);
        limiter.activate(1 << 20);
        assert_growing(&mut limiter, (1 << 20) + (4 << 20));
        assert_eq!(limiter.peak(), 4 << 20);
    }
}
