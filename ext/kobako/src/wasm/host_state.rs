//! Per-Store host state shared with every wasmtime callback.
//!
//! Owned by [`StoreCell`] (a `RefCell` shim wrapping `wasmtime::Store`)
//! and threaded through every host import — the `__kobako_dispatch`
//! dispatcher reads the server handle, while the run-path methods on
//! [`crate::wasm::Instance`] install fresh WASI context + pipes before
//! every `#run` (SPEC.md B-03 / B-04).
//!
//! The state also carries the per-run wall-clock deadline (SPEC.md B-01,
//! E-19) and the linear-memory cap [`KobakoLimiter`] (SPEC.md B-01,
//! E-20). Both are read from the wasmtime `epoch_deadline_callback` /
//! `ResourceLimiter` callbacks installed in
//! [`crate::wasm::Instance::from_path`].

use std::cell::{Ref, RefCell, RefMut};
use std::time::Instant;

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
    server: Option<Opaque<Value>>,
    deadline: Option<Instant>,
    limiter: KobakoLimiter,
}

impl HostState {
    /// Build a fresh per-Store host state. `memory_limit` carries the
    /// `Sandbox#memory_limit` cap in bytes (or `None` to disable the cap);
    /// it is read from the wasmtime [`ResourceLimiter`] callback every
    /// time the guest grows linear memory (SPEC.md B-01, E-20).
    pub(super) fn new(memory_limit: Option<usize>) -> Self {
        Self {
            wasi: None,
            stdout_pipe: None,
            stderr_pipe: None,
            server: None,
            deadline: None,
            limiter: KobakoLimiter::new(memory_limit),
        }
    }

    /// Install a freshly-built WASI context plus the matching stdout/stderr
    /// pipe clones. Called from [`crate::wasm::Instance::run`] at the top
    /// of every guest invocation (SPEC.md B-03 / B-04).
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

    /// Bind the Ruby-side `Kobako::RPC::Server` handle. From this point on,
    /// every `__kobako_dispatch` host import invocation routes through it.
    pub(super) fn bind_server(&mut self, server: Opaque<Value>) {
        self.server = Some(server);
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

    /// Return the bound Server handle. `Opaque<Value>` is `Copy`, so the
    /// handle is returned by value rather than by reference. None means no
    /// Server has been bound yet via [`HostState::bind_server`].
    pub(super) fn server(&self) -> Option<Opaque<Value>> {
        self.server
    }

    /// Mutable handle to the live WASI context. Panics if no context has
    /// been installed yet — every call site is downstream of
    /// [`HostState::install_wasi`] running at the top of `Instance::run`,
    /// so reaching this branch with `None` signals a host-side wiring bug.
    pub(super) fn wasi_mut(&mut self) -> &mut WasiP1Ctx {
        self.wasi
            .as_mut()
            .expect("WASI context not initialised — call Instance#run before any WASI use")
    }

    /// Replace the per-run wall-clock deadline. `Some(at)` makes the
    /// epoch-deadline callback trap once `Instant::now() >= at`; `None`
    /// disables the cap. Called at the top of every `#run` (SPEC.md B-01).
    pub(super) fn set_deadline(&mut self, deadline: Option<Instant>) {
        self.deadline = deadline;
    }

    /// Return the current per-run deadline. Read from the epoch-deadline
    /// callback installed by [`crate::wasm::Instance::from_path`].
    pub(super) fn deadline(&self) -> Option<Instant> {
        self.deadline
    }

    /// Mutable handle to the embedded [`KobakoLimiter`]. Shared by the
    /// wasmtime [`ResourceLimiter`] callback (set once at Store build
    /// time) and by [`crate::wasm::Instance`] for arming / disarming the
    /// memory cap around each guest run. Same shape as
    /// [`HostState::wasi_mut`] — callers operate on the inner type
    /// directly instead of going through a per-action passthrough.
    pub(super) fn limiter_mut(&mut self) -> &mut KobakoLimiter {
        &mut self.limiter
    }
}

/// Resource limiter that enforces the `memory_limit` cap from SPEC.md
/// B-01 / E-20 on every guest `memory.grow`.
///
/// `max_memory` is the byte cap (`None` disables the cap). `cap_active`
/// gates whether the cap is enforced — wasmtime's `ResourceLimiter`
/// fires for both the module's declared initial allocation and every
/// subsequent `memory.grow`, but SPEC.md E-20 scopes the trap to
/// `memory.grow` specifically. [`KobakoLimiter::activate`] /
/// [`KobakoLimiter::deactivate`] flip the flag for the lifetime of an
/// `Instance::run` call. When `cap_active` is `false`, the limiter
/// always allows growth.
///
/// When `memory.grow` would push linear memory past the cap, the
/// limiter returns [`MemoryLimitTrap`] from `memory_growing`; wasmtime
/// turns that into the trap surfaced to the host as `__kobako_run`
/// failure.
#[derive(Debug, Clone, Copy)]
pub(super) struct KobakoLimiter {
    max_memory: Option<usize>,
    cap_active: bool,
}

impl KobakoLimiter {
    fn new(max_memory: Option<usize>) -> Self {
        Self {
            max_memory,
            cap_active: false,
        }
    }

    /// Arm the cap so subsequent `memory.grow` calls are checked
    /// against `memory_limit`. The cap is dormant by default — the
    /// module's declared initial memory is allocated during
    /// `Linker::instantiate` and SPEC.md E-20 scopes the trap to
    /// `memory.grow` (not the instantiation-time initial allocation).
    /// [`crate::wasm::Instance::run`] calls this right before
    /// `__kobako_run`.
    pub(super) fn activate(&mut self) {
        self.cap_active = true;
    }

    /// Disarm the cap so post-run host bookkeeping (e.g. fetching the
    /// OUTCOME_BUFFER, which can grow guest memory transiently) is
    /// not attributed to the user script. Paired with
    /// [`KobakoLimiter::activate`].
    pub(super) fn deactivate(&mut self) {
        self.cap_active = false;
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
        if let Some(limit) = self.max_memory {
            if desired > limit {
                return Err(wasmtime::Error::new(MemoryLimitTrap { desired, limit }));
            }
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
/// SPEC.md E-20. Downcast from the wasmtime trap error to surface as
/// `Kobako::Wasm::MemoryLimitError` on the Ruby side. Callers use the
/// `Display` impl below — no field is read directly — so the inner
/// state stays private.
#[derive(Debug)]
pub(crate) struct MemoryLimitTrap {
    desired: usize,
    limit: usize,
}

impl std::fmt::Display for MemoryLimitTrap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "guest memory.grow would exceed memory_limit: desired={} bytes, limit={} bytes",
            self.desired, self.limit
        )
    }
}

impl std::error::Error for MemoryLimitTrap {}

/// Marker error returned from the epoch-deadline callback on SPEC.md
/// E-19. Downcast from the wasmtime trap error to surface as
/// `Kobako::Wasm::TimeoutError` on the Ruby side.
#[derive(Debug)]
pub(crate) struct TimeoutTrap;

impl std::fmt::Display for TimeoutTrap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "guest exceeded the configured wall-clock timeout")
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

// SAFETY: Ruby's GVL serialises access to magnus-wrapped objects on a single
// OS thread at a time. `wasmtime::Store` is `Send` (verified upstream); the
// `RefCell`-mediated mutation is therefore safe under the GVL invariant.
unsafe impl Send for StoreCell {}
unsafe impl Sync for StoreCell {}
