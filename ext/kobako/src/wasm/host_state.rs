//! Per-Store host state shared with every wasmtime callback.
//!
//! Owned by [`StoreCell`] (a `RefCell` shim wrapping `wasmtime::Store`)
//! and threaded through every host import — the `__kobako_dispatch`
//! dispatcher reads the server handle, while the run-path methods on
//! [`crate::wasm::Instance`] install fresh WASI context + pipes before
//! every `#run` (docs/behavior.md B-03 / B-04).
//!
//! The state also carries the per-invocation wall-clock deadline
//! (docs/behavior.md B-01, E-19) and the per-invocation linear-memory
//! delta cap [`KobakoLimiter`] (docs/behavior.md B-01, E-20). Both are
//! read from the wasmtime `epoch_deadline_callback` / `ResourceLimiter`
//! callbacks installed in [`crate::wasm::Instance::from_path`]. The
//! memory cap measures only the `memory.grow` delta past the linear-
//! memory size captured at invocation entry — the mruby image's
//! initial allocation and prior invocations' watermark are outside the
//! budget.

use std::cell::{Ref, RefCell, RefMut};
use std::time::{Duration, Instant};

use magnus::{value::Opaque, Value};
use wasmtime::{ResourceLimiter, Store as WtStore};
use wasmtime_wasi::p1::WasiP1Ctx;
use wasmtime_wasi::p2::pipe::MemoryOutputPipe;

/// Per-Store host state threaded through every host import callback.
///
/// All field access is mediated by methods on this type — the WASI ctx is
/// rebuilt fresh before each `#run` via [`HostState::install_wasi`], the
/// Ruby Server handle is set once via [`HostState::bind_server`], and
/// captured stdout/stderr bytes are read after the run via
/// [`HostState::stdout_bytes`] / [`HostState::stderr_bytes`]. The fields
/// are private so the mutation surface stays narrow.
pub(super) struct HostState {
    wasi: Option<WasiP1Ctx>,
    stdout_pipe: Option<MemoryOutputPipe>,
    stderr_pipe: Option<MemoryOutputPipe>,
    channel: Option<Opaque<Value>>,
    deadline: Option<Instant>,
    limiter: KobakoLimiter,
    wall_entry: Option<Instant>,
    wall_time: Duration,
}

impl HostState {
    /// Build a fresh per-Store host state. `memory_limit` carries the
    /// `Sandbox#memory_limit` cap in bytes (or `None` to disable the cap);
    /// it is read from the wasmtime [`ResourceLimiter`] callback every
    /// time the guest grows linear memory (docs/behavior.md B-01, E-20).
    pub(super) fn new(memory_limit: Option<usize>) -> Self {
        Self {
            wasi: None,
            stdout_pipe: None,
            stderr_pipe: None,
            channel: None,
            deadline: None,
            limiter: KobakoLimiter::new(memory_limit),
            wall_entry: None,
            wall_time: Duration::ZERO,
        }
    }

    /// Install a freshly-built WASI context plus the matching stdout/stderr
    /// pipe clones. Called from [`crate::wasm::Instance::eval`] /
    /// [`crate::wasm::Instance::run`] at the top of every guest
    /// invocation (docs/behavior.md B-03 / B-04).
    pub(super) fn install_wasi(
        &mut self,
        wasi: WasiP1Ctx,
        stdout: MemoryOutputPipe,
        stderr: MemoryOutputPipe,
    ) {
        self.wasi = Some(wasi);
        self.stdout_pipe = Some(stdout);
        self.stderr_pipe = Some(stderr);
    }

    /// Bind the Ruby-side `Kobako::Transport::Channel` handle. From this
    /// point on, every `__kobako_dispatch` host import invocation
    /// routes through it.
    pub(super) fn bind_channel(&mut self, channel: Opaque<Value>) {
        self.channel = Some(channel);
    }

    /// Snapshot the bytes captured on guest fd 1 during the most recent
    /// run. Empty vec before any run.
    pub(super) fn stdout_bytes(&self) -> Vec<u8> {
        self.stdout_pipe
            .as_ref()
            .map(|p| p.contents().to_vec())
            .unwrap_or_default()
    }

    /// Snapshot the bytes captured on guest fd 2 during the most recent
    /// run. Empty vec before any run.
    pub(super) fn stderr_bytes(&self) -> Vec<u8> {
        self.stderr_pipe
            .as_ref()
            .map(|p| p.contents().to_vec())
            .unwrap_or_default()
    }

    /// Return the bound Channel handle. `Opaque<Value>` is `Copy`, so
    /// the handle is returned by value rather than by reference. None
    /// means no Channel has been bound yet via
    /// [`HostState::bind_channel`].
    pub(super) fn channel(&self) -> Option<Opaque<Value>> {
        self.channel
    }

    /// Mutable handle to the live WASI context. Panics if no context has
    /// been installed yet — every call site is downstream of
    /// [`HostState::install_wasi`] running at the top of
    /// `Instance::eval` / `Instance::run`, so reaching this branch with
    /// `None` signals a host-side wiring bug.
    pub(super) fn wasi_mut(&mut self) -> &mut WasiP1Ctx {
        self.wasi.as_mut().expect(
            "WASI context not initialised — call Instance#eval / Instance#run before any WASI use",
        )
    }

    /// Replace the per-run wall-clock deadline. `Some(at)` makes the
    /// epoch-deadline callback trap once `Instant::now() >= at`; `None`
    /// disables the cap. Called at the top of every `#run` (docs/behavior.md B-01).
    pub(super) fn set_deadline(&mut self, deadline: Option<Instant>) {
        self.deadline = deadline;
    }

    /// Return the current per-run deadline. Read from the epoch-deadline
    /// callback installed by [`crate::wasm::Instance::from_path`].
    pub(super) fn deadline(&self) -> Option<Instant> {
        self.deadline
    }

    /// Mutable handle to the embedded [`KobakoLimiter`]. Required by
    /// the wasmtime [`ResourceLimiter`] callback wiring in
    /// [`crate::wasm::Instance::from_path`]
    /// (`store.limiter(|state| state.limiter_mut())`); kept private to
    /// the wasm submodule so the only public surface for arming the
    /// cap goes through [`HostState::arm_memory_cap`] /
    /// [`HostState::disarm_memory_cap`].
    pub(super) fn limiter_mut(&mut self) -> &mut KobakoLimiter {
        &mut self.limiter
    }

    /// Arm the docs/behavior.md E-20 memory cap for one guest run with
    /// the current linear-memory size as the baseline. The limiter
    /// charges only the `memory.grow` delta past `baseline` against
    /// the cap, so the mruby image's initial allocation and the
    /// high-water mark left by prior invocations do not consume the
    /// budget. Paired with [`HostState::disarm_memory_cap`] around the
    /// call to the corresponding `__kobako_*` export so post-run host
    /// bookkeeping (e.g. fetching the OUTCOME_BUFFER) is not
    /// attributed to the user script.
    pub(super) fn arm_memory_cap(&mut self, baseline: usize) {
        self.limiter.activate(baseline);
    }

    /// Disarm the docs/behavior.md E-20 memory cap. See
    /// [`HostState::arm_memory_cap`].
    pub(super) fn disarm_memory_cap(&mut self) {
        self.limiter.deactivate();
    }

    /// Stamp the wall-clock entry instant for the docs/behavior.md
    /// B-35 `wall_time` measurement. Called at the top of every
    /// invocation immediately before the guest export call so the
    /// bracket matches the `timeout` deadline accounting (B-01) and
    /// excludes post-run host bookkeeping such as `OUTCOME_BUFFER`
    /// decoding.
    pub(super) fn start_wall_clock(&mut self) {
        self.wall_entry = Some(Instant::now());
    }

    /// Close the docs/behavior.md B-35 `wall_time` measurement
    /// started by [`HostState::start_wall_clock`]. Idempotent — a
    /// stop with no matching start (e.g. if the guest export call
    /// never executed because of a host-side allocation failure)
    /// leaves the previously-recorded value untouched.
    pub(super) fn stop_wall_clock(&mut self) {
        if let Some(entry) = self.wall_entry.take() {
            self.wall_time = entry.elapsed();
        }
    }

    /// Return the wall-clock duration the most recent invocation
    /// spent inside the guest export call (docs/behavior.md B-35).
    /// Zero before the first invocation.
    pub(super) fn wall_time(&self) -> Duration {
        self.wall_time
    }

    /// Return the docs/behavior.md B-35 `memory_peak` — the high-
    /// water mark of the per-invocation `memory.grow` delta past the
    /// linear-memory size captured at invocation entry. Zero before
    /// the first invocation.
    pub(super) fn memory_peak(&self) -> usize {
        self.limiter.peak()
    }
}

/// Resource limiter that enforces the per-invocation `memory_limit`
/// cap from docs/behavior.md B-01 / E-20.
///
/// `max_memory` is the byte cap on per-invocation growth (`None` disables
/// the cap). `baseline` is the linear-memory size captured at invocation
/// entry by [`KobakoLimiter::activate`]; the limiter charges only the
/// `memory.grow` delta past `baseline` against `max_memory`, so the
/// mruby image's initial allocation and any high-water mark left by
/// prior invocations on the same Sandbox do not consume the budget.
/// `cap_active` gates whether the cap is enforced — wasmtime's
/// `ResourceLimiter` also fires for the module's declared initial
/// allocation at instantiation time, but the cap stays dormant until
/// [`KobakoLimiter::activate`] flips the flag for one
/// `Instance::eval` / `Instance::run` call. When `cap_active` is
/// `false`, the limiter always allows growth.
///
/// When `memory.grow` would push the per-invocation delta past
/// `max_memory`, the limiter returns [`MemoryLimitTrap`] from
/// `memory_growing`; wasmtime turns that into the trap surfaced to the
/// host as a guest invocation failure.
#[derive(Debug, Clone, Copy)]
pub(super) struct KobakoLimiter {
    max_memory: Option<usize>,
    baseline: usize,
    cap_active: bool,
    peak: usize,
}

impl KobakoLimiter {
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
    /// [`HostState::arm_memory_cap`] at the top of every invocation;
    /// the cap is dormant by default — the module's declared initial
    /// memory is allocated during `Linker::instantiate` and the
    /// per-invocation budget excludes anything that existed before
    /// arming (docs/behavior.md B-01 Notes, E-20). Also clears the
    /// per-invocation [`KobakoLimiter::peak`] high-water so the
    /// docs/behavior.md B-35 `memory_peak` accounting restarts from
    /// zero for the new invocation.
    fn activate(&mut self, baseline: usize) {
        self.baseline = baseline;
        self.cap_active = true;
        self.peak = 0;
    }

    /// Disarm the cap so post-run host bookkeeping (e.g. fetching the
    /// OUTCOME_BUFFER, which can grow guest memory transiently) is
    /// not attributed to the user script. Paired with
    /// [`KobakoLimiter::activate`].
    fn deactivate(&mut self) {
        self.cap_active = false;
    }

    /// Return the high-water mark of the per-invocation
    /// `memory.grow` delta past `baseline` observed since the last
    /// [`KobakoLimiter::activate`]. Read after the guest export
    /// returns to populate `Kobako::Usage#memory_peak`
    /// (docs/behavior.md B-35). Pinned to the last accepted grow —
    /// rejected `desired` values that trip the docs/behavior.md E-20
    /// cap never update the peak, so the reported value never exceeds
    /// `memory_limit`.
    pub(super) fn peak(&self) -> usize {
        self.peak
    }
}

impl ResourceLimiter for KobakoLimiter {
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

/// Marker error returned from [`KobakoLimiter::memory_growing`] on
/// docs/behavior.md E-20. Downcast from the wasmtime trap error to surface as
/// `Kobako::MemoryLimitError` on the Ruby side. Callers use the
/// `Display` impl below — no field is read directly — so the inner
/// state stays private.
#[derive(Debug)]
pub(crate) struct MemoryLimitTrap {
    desired: usize,
    limit: usize,
}

impl MemoryLimitTrap {
    /// Construct a trap with the given +desired+ / +limit+ pair. Used
    /// internally by [`KobakoLimiter::memory_growing`] in production and
    /// by the sibling-module +classify_trap+ unit tests to materialise
    /// a representative error for downcast routing.
    #[cfg(test)]
    pub(super) fn new(desired: usize, limit: usize) -> Self {
        Self { desired, limit }
    }
}

impl std::fmt::Display for MemoryLimitTrap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "linear memory growth exceeded memory_limit: \
             desired={} bytes, limit={} bytes",
            self.desired, self.limit
        )
    }
}

impl std::error::Error for MemoryLimitTrap {}

/// Marker error returned from the epoch-deadline callback on
/// docs/behavior.md E-19. Downcast from the wasmtime trap error to
/// surface as `Kobako::TimeoutError` on the Ruby side.
#[derive(Debug)]
pub(crate) struct TimeoutTrap;

impl std::fmt::Display for TimeoutTrap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "wall-clock deadline exceeded")
    }
}

impl std::error::Error for TimeoutTrap {}

/// Interior-mutability wrapper around `wasmtime::Store<HostState>`.
///
/// Magnus requires `Send + Sync` for wrapped types. `wasmtime::Store` is not
/// `Sync`, so we wrap it in a `RefCell`. `RefCell` alone is sufficient
/// because magnus enforces single-threaded GVL access from Ruby; `Send` and
/// `Sync` are asserted via the unsafe impls below.
pub(super) struct StoreCell {
    inner: RefCell<WtStore<HostState>>,
}

impl StoreCell {
    /// Wrap a freshly-built `wasmtime::Store<HostState>` so it can be owned
    /// by the magnus-wrapped `Instance`.
    pub(super) fn new(store: WtStore<HostState>) -> Self {
        Self {
            inner: RefCell::new(store),
        }
    }

    /// Immutable borrow of the wrapped Store. Panics if a `borrow_mut()`
    /// is currently live — matches `RefCell::borrow` semantics.
    pub(super) fn borrow(&self) -> Ref<'_, WtStore<HostState>> {
        self.inner.borrow()
    }

    /// Mutable borrow of the wrapped Store. Panics if any other borrow is
    /// currently live — matches `RefCell::borrow_mut` semantics.
    pub(super) fn borrow_mut(&self) -> RefMut<'_, WtStore<HostState>> {
        self.inner.borrow_mut()
    }
}

// SAFETY: magnus requires `Send + Sync` on `#[magnus::wrap]` types. Both
// claims hold under the GVL invariant:
//
//   * Send — `wasmtime::Store<HostState>` is itself `Send` (verified
//     upstream by wasmtime; see `wasmtime::Store`'s trait impls).
//     `RefCell<T>: Send` whenever `T: Send`. The remaining stored state
//     (`HostState`) holds `Opaque<Value>` for the Ruby Server handle —
//     `Opaque<Value>` is documented as `Send` by magnus precisely so
//     wrapped objects can satisfy this bound.
//
//   * Sync — `RefCell` is *not* `Sync` in the general Rust sense
//     (concurrent `borrow_mut` is UB). We assert `Sync` here because the
//     GVL serialises every call into Ruby C and every entry into magnus-
//     wrapped methods onto a single OS thread at a time: by the time the
//     `Sync` bound matters, magnus has already established that only one
//     thread can be inside the wrapper. Cross-thread mutation cannot
//     occur. If a future magnus release adopts a thread model that
//     permits concurrent access to wrapped objects, this assertion would
//     have to revert and `StoreCell` would need to switch to
//     `Mutex<wasmtime::Store<…>>` — but as of magnus 0.8 the contract
//     holds.
unsafe impl Send for StoreCell {}
unsafe impl Sync for StoreCell {}

#[cfg(test)]
mod tests {
    //! Unit tests for [`KobakoLimiter`] — the per-invocation memory
    //! delta cap. The Ruby-facing E2E suite exercises the full path
    //! through wasmtime; these tests pin the pure delta arithmetic so
    //! a regression that breaks the baseline accounting (e.g. dropping
    //! the `baseline` subtraction, or letting `activate` carry stale
    //! state across invocations) is caught without spinning up a
    //! Store.
    use super::{KobakoLimiter, MemoryLimitTrap};
    use wasmtime::ResourceLimiter;

    fn assert_growing(limiter: &mut KobakoLimiter, desired: usize) {
        assert!(
            limiter.memory_growing(0, desired, None).unwrap(),
            "expected memory_growing({desired}) to allow growth"
        );
    }

    fn assert_trapping(limiter: &mut KobakoLimiter, desired: usize) {
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
        let mut limiter = KobakoLimiter::new(Some(1 << 20));
        // Without `activate`, the cap is dormant — the module's
        // declared initial allocation must pass through unconditionally.
        assert_growing(&mut limiter, 100 << 20);
    }

    #[test]
    fn delta_below_cap_passes_after_activate() {
        let mut limiter = KobakoLimiter::new(Some(1 << 20));
        limiter.activate(2 << 20);
        // baseline=2 MiB, desired=2.5 MiB → delta=0.5 MiB ≤ 1 MiB cap.
        assert_growing(&mut limiter, (2 << 20) + (1 << 19));
    }

    #[test]
    fn delta_past_cap_traps_with_memory_limit_trap() {
        let mut limiter = KobakoLimiter::new(Some(1 << 20));
        limiter.activate(2 << 20);
        // baseline=2 MiB, desired=4 MiB → delta=2 MiB > 1 MiB cap.
        assert_trapping(&mut limiter, 4 << 20);
    }

    #[test]
    fn activate_resets_baseline_on_each_invocation() {
        let mut limiter = KobakoLimiter::new(Some(1 << 20));
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
        let mut limiter = KobakoLimiter::new(None);
        limiter.activate(0);
        assert_growing(&mut limiter, 100 << 20);
    }

    #[test]
    fn peak_starts_at_zero_before_any_grow() {
        let limiter = KobakoLimiter::new(Some(1 << 20));
        assert_eq!(limiter.peak(), 0);
    }

    #[test]
    fn peak_tracks_high_water_of_delta_past_baseline() {
        let mut limiter = KobakoLimiter::new(Some(1 << 20));
        limiter.activate(2 << 20);
        assert_growing(&mut limiter, (2 << 20) + (1 << 18)); // delta=256 KiB
        assert_growing(&mut limiter, (2 << 20) + (1 << 19)); // delta=512 KiB (new peak)
        assert_growing(&mut limiter, (2 << 20) + (1 << 17)); // delta=128 KiB (below peak)
        assert_eq!(limiter.peak(), 1 << 19);
    }

    #[test]
    fn trap_does_not_update_peak() {
        let mut limiter = KobakoLimiter::new(Some(1 << 20));
        limiter.activate(2 << 20);
        assert_growing(&mut limiter, (2 << 20) + (1 << 19)); // delta=512 KiB
        assert_trapping(&mut limiter, (2 << 20) + (2 << 20)); // would be 2 MiB > 1 MiB cap
                                                              // Peak reflects the last accepted grow, not the rejected desired.
        assert_eq!(limiter.peak(), 1 << 19);
    }

    #[test]
    fn activate_resets_peak_for_new_invocation() {
        let mut limiter = KobakoLimiter::new(Some(1 << 20));
        limiter.activate(2 << 20);
        assert_growing(&mut limiter, (2 << 20) + (1 << 19));
        assert_eq!(limiter.peak(), 1 << 19);
        limiter.activate(3 << 20);
        assert_eq!(limiter.peak(), 0);
    }

    #[test]
    fn disabled_cap_still_tracks_peak() {
        let mut limiter = KobakoLimiter::new(None);
        limiter.activate(1 << 20);
        assert_growing(&mut limiter, (1 << 20) + (4 << 20));
        assert_eq!(limiter.peak(), 4 << 20);
    }
}
