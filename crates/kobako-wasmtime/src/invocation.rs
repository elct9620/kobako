//! Per-invocation host state — the materialised
//! [SPEC.md Single-Invocation Slot] (one `Invocation` per OS thread
//! for the lifetime of one `Driver` invoke call).
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
//!
//! [SPEC.md Single-Invocation Slot]: ../../../SPEC.md

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
}

impl Invocation {
    /// Build a fresh per-Store host state. `memory_limit` carries the
    /// `Sandbox#memory_limit` cap in bytes (or `None` to disable the cap);
    /// it is read from the wasmtime `ResourceLimiter` callback every
    /// time the guest grows linear memory.
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
        }
    }

    /// Install a freshly-built WASI context plus the matching stdout/stderr
    /// pipe clones. Called from `frames::install_wasi_frames`, which
    /// `Driver::invoke` runs at the top of every guest
    /// invocation.
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

    /// Bind the dispatch handler for this invocation. From this point on,
    /// every `__kobako_dispatch` host import invocation hands the handler
    /// the request bytes and expects encoded Response bytes back.
    pub(crate) fn bind_on_dispatch(&mut self, handler: Arc<dyn DispatchHandler>) {
        self.on_dispatch = Some(handler);
    }

    /// Snapshot the bytes captured on guest fd 1 during the most recent
    /// run. Empty vec before any run.
    pub(crate) fn stdout_bytes(&self) -> Vec<u8> {
        self.stdout_pipe
            .as_ref()
            .map(|p| p.contents().to_vec())
            .unwrap_or_default()
    }

    /// Snapshot the bytes captured on guest fd 2 during the most recent
    /// run. Empty vec before any run.
    pub(crate) fn stderr_bytes(&self) -> Vec<u8> {
        self.stderr_pipe
            .as_ref()
            .map(|p| p.contents().to_vec())
            .unwrap_or_default()
    }

    /// Return a clone of the bound dispatch handler (an `Arc`, so the clone
    /// is a cheap refcount bump). Cloning releases the borrow on the
    /// `Caller` so the dispatcher can re-borrow it to write the response.
    /// None means no handler has been bound yet via
    /// `Invocation::bind_on_dispatch`.
    pub(crate) fn on_dispatch(&self) -> Option<Arc<dyn DispatchHandler>> {
        self.on_dispatch.clone()
    }

    /// Mutable handle to the live WASI context. Panics if no context has
    /// been installed yet — every call site is downstream of
    /// `Invocation::install_wasi` running at the top of every `Driver`
    /// invoke, so reaching this branch with `None` signals a host-side
    /// wiring bug.
    pub(crate) fn wasi_mut(&mut self) -> &mut WasiP1Ctx {
        self.wasi.as_mut().expect(
            "WASI context not initialised — the driver must install frames before any WASI use",
        )
    }

    /// Replace the per-run wall-clock deadline. `Some(at)` makes the
    /// epoch-deadline callback trap once `Instant::now() >= at`; `None`
    /// disables the cap. Called from `Driver::prime_caps` at the top of
    /// every invocation (`#eval` and `#run`).
    pub(crate) fn set_deadline(&mut self, deadline: Option<Instant>) {
        self.deadline = deadline;
    }

    /// Return the current per-run deadline. Read from the epoch-deadline
    /// callback installed by `Driver::new_store`.
    pub(crate) fn deadline(&self) -> Option<Instant> {
        self.deadline
    }

    /// Mutable handle to the embedded `MemoryLimiter`. Required by
    /// the wasmtime `ResourceLimiter` callback wiring in
    /// `Driver::new_store`
    /// (`store.limiter(|state| state.limiter_mut())`); kept private to
    /// the wasm submodule so the only public surface for arming the
    /// cap goes through `Invocation::arm_memory_cap` /
    /// `Invocation::disarm_memory_cap`.
    pub(crate) fn limiter_mut(&mut self) -> &mut MemoryLimiter {
        &mut self.limiter
    }

    /// Arm the memory cap for one guest run with
    /// the current linear-memory size as the baseline. The limiter
    /// charges only the `memory.grow` delta past `baseline` against
    /// the cap, so the mruby image's initial allocation and the
    /// high-water mark left by prior invocations do not consume the
    /// budget. Paired with `Invocation::disarm_memory_cap` around the
    /// call to the corresponding `__kobako_*` export so post-run host
    /// bookkeeping (e.g. fetching the OUTCOME_BUFFER) is not
    /// attributed to the user script.
    pub(crate) fn arm_memory_cap(&mut self, baseline: usize) {
        self.limiter.activate(baseline);
    }

    /// Disarm the memory cap. See
    /// `Invocation::arm_memory_cap`.
    pub(crate) fn disarm_memory_cap(&mut self) {
        self.limiter.deactivate();
    }

    /// Stamp the wall-clock entry instant for the `wall_time`
    /// measurement. Called at the top of every
    /// invocation immediately before the guest export call so the
    /// bracket matches the `timeout` deadline accounting and
    /// excludes post-run host bookkeeping such as `OUTCOME_BUFFER`
    /// decoding.
    pub(crate) fn start_wall_clock(&mut self) {
        self.wall_entry = Some(Instant::now());
    }

    /// Close the `wall_time` measurement
    /// started by `Invocation::start_wall_clock`. Idempotent — a
    /// stop with no matching start (e.g. if the guest export call
    /// never executed because of a host-side allocation failure)
    /// leaves the previously-recorded value untouched.
    pub(crate) fn stop_wall_clock(&mut self) {
        if let Some(entry) = self.wall_entry.take() {
            self.wall_time = entry.elapsed();
        }
    }

    /// Return the wall-clock duration the most recent invocation
    /// spent inside the guest export call.
    /// Zero before the first invocation.
    pub(crate) fn wall_time(&self) -> Duration {
        self.wall_time
    }

    /// Return the `memory_peak` — the high-
    /// water mark of the per-invocation `memory.grow` delta past the
    /// linear-memory size captured at invocation entry. Zero before
    /// the first invocation.
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

    /// Arm the cap so subsequent `memory.grow` calls are charged
    /// against `max_memory` starting from `baseline` bytes. Called via
    /// `Invocation::arm_memory_cap` at the top of every invocation;
    /// the cap is dormant by default — the module's declared initial
    /// memory is allocated during `Linker::instantiate` and the
    /// per-invocation budget excludes anything that existed before
    /// arming. Also clears the
    /// per-invocation `MemoryLimiter::peak` high-water so the
    /// `memory_peak` accounting restarts from
    /// zero for the new invocation.
    fn activate(&mut self, baseline: usize) {
        self.baseline = baseline;
        self.cap_active = true;
        self.peak = 0;
    }

    /// Disarm the cap so post-run host bookkeeping (e.g. fetching the
    /// OUTCOME_BUFFER, which can grow guest memory transiently) is
    /// not attributed to the user script. Paired with
    /// `MemoryLimiter::activate`.
    fn deactivate(&mut self) {
        self.cap_active = false;
    }

    /// Return the high-water mark of the per-invocation
    /// `memory.grow` delta past `baseline` observed since the last
    /// `MemoryLimiter::activate`. Read after the guest export
    /// returns to populate `Kobako::Usage#memory_peak`.
    /// Pinned to the last accepted grow —
    /// rejected `desired` values that trip the memory
    /// cap never update the peak, so the reported value never exceeds
    /// `memory_limit`.
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
/// trap error to surface as
/// `Kobako::MemoryLimitError` on the Ruby side. Callers use the
/// `Display` impl below — no field is read directly — so the inner
/// state stays private.
#[derive(Debug)]
pub(crate) struct MemoryLimitTrap {
    desired: usize,
    limit: usize,
}

impl MemoryLimitTrap {
    /// Construct a trap with the given `desired` / `limit` pair. Used
    /// internally by `MemoryLimiter::memory_growing` in production and
    /// by the sibling-module `classify_trap` unit tests to materialise
    /// a representative error for downcast routing.
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
/// error to surface as `Kobako::TimeoutError` on the Ruby side.
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

    #[test]
    fn dormant_limiter_allows_any_growth() {
        let mut limiter = MemoryLimiter::new(Some(1 << 20));
        // Without `activate`, the cap is dormant — the module's
        // declared initial allocation must pass through unconditionally.
        assert_growing(&mut limiter, 100 << 20);
    }

    #[test]
    fn delta_below_cap_passes_after_activate() {
        let mut limiter = MemoryLimiter::new(Some(1 << 20));
        limiter.activate(2 << 20);
        // baseline=2 MiB, desired=2.5 MiB → delta=0.5 MiB ≤ 1 MiB cap.
        assert_growing(&mut limiter, (2 << 20) + (1 << 19));
    }

    #[test]
    fn delta_past_cap_traps_with_memory_limit_trap() {
        let mut limiter = MemoryLimiter::new(Some(1 << 20));
        limiter.activate(2 << 20);
        // baseline=2 MiB, desired=4 MiB → delta=2 MiB > 1 MiB cap.
        assert_trapping(&mut limiter, 4 << 20);
    }

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

    #[test]
    fn disabled_cap_ignores_delta_size() {
        let mut limiter = MemoryLimiter::new(None);
        limiter.activate(0);
        assert_growing(&mut limiter, 100 << 20);
    }

    #[test]
    fn peak_starts_at_zero_before_any_grow() {
        let limiter = MemoryLimiter::new(Some(1 << 20));
        assert_eq!(limiter.peak(), 0);
    }

    #[test]
    fn peak_tracks_high_water_of_delta_past_baseline() {
        let mut limiter = MemoryLimiter::new(Some(1 << 20));
        limiter.activate(2 << 20);
        assert_growing(&mut limiter, (2 << 20) + (1 << 18)); // delta=256 KiB
        assert_growing(&mut limiter, (2 << 20) + (1 << 19)); // delta=512 KiB (new peak)
        assert_growing(&mut limiter, (2 << 20) + (1 << 17)); // delta=128 KiB (below peak)
        assert_eq!(limiter.peak(), 1 << 19);
    }

    #[test]
    fn trap_does_not_update_peak() {
        let mut limiter = MemoryLimiter::new(Some(1 << 20));
        limiter.activate(2 << 20);
        assert_growing(&mut limiter, (2 << 20) + (1 << 19)); // delta=512 KiB
        assert_trapping(&mut limiter, (2 << 20) + (2 << 20)); // would be 2 MiB > 1 MiB cap
                                                              // Peak reflects the last accepted grow, not the rejected desired.
        assert_eq!(limiter.peak(), 1 << 19);
    }

    #[test]
    fn activate_resets_peak_for_new_invocation() {
        let mut limiter = MemoryLimiter::new(Some(1 << 20));
        limiter.activate(2 << 20);
        assert_growing(&mut limiter, (2 << 20) + (1 << 19));
        assert_eq!(limiter.peak(), 1 << 19);
        limiter.activate(3 << 20);
        assert_eq!(limiter.peak(), 0);
    }

    #[test]
    fn disabled_cap_still_tracks_peak() {
        let mut limiter = MemoryLimiter::new(None);
        limiter.activate(1 << 20);
        assert_growing(&mut limiter, (1 << 20) + (4 << 20));
        assert_eq!(limiter.peak(), 4 << 20);
    }
}
