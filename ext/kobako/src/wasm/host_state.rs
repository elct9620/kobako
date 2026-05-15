//! Per-Store host state shared with every wasmtime callback.
//!
//! Owned by [`StoreCell`] (a `RefCell` shim wrapping `wasmtime::Store`)
//! and threaded through every host import — the `__kobako_dispatch`
//! dispatcher reads the registry handle, while the run-path methods on
//! [`crate::wasm::Instance`] install fresh WASI context + pipes before
//! every `#run` (SPEC.md B-03 / B-04).

use std::cell::{Ref, RefCell, RefMut};

use magnus::{value::Opaque, Value};
use wasmtime::Store as WtStore;
use wasmtime_wasi::p1::WasiP1Ctx;
use wasmtime_wasi::p2::pipe::MemoryOutputPipe;

/// Per-Store host state threaded through every host import callback.
///
/// All field access is mediated by methods on this type — the WASI ctx is
/// rebuilt fresh before each `#run` via [`HostState::install_wasi`], the
/// Ruby Registry handle is set once via [`HostState::bind_registry`], and
/// captured stdout/stderr bytes are read after the run via
/// [`HostState::stdout_bytes`] / [`HostState::stderr_bytes`]. The fields
/// are private so the mutation surface stays narrow.
#[derive(Default)]
pub(super) struct HostState {
    wasi: Option<WasiP1Ctx>,
    stdout_pipe: Option<MemoryOutputPipe>,
    stderr_pipe: Option<MemoryOutputPipe>,
    registry: Option<Opaque<Value>>,
}

impl HostState {
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

    /// Bind the Ruby-side `Kobako::Registry` handle. From this point on,
    /// every `__kobako_dispatch` host import invocation routes through it.
    pub(super) fn bind_registry(&mut self, registry: Opaque<Value>) {
        self.registry = Some(registry);
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

    /// Return the bound Registry handle. `Opaque<Value>` is `Copy`, so the
    /// handle is returned by value rather than by reference. None means no
    /// Registry has been bound yet via [`HostState::bind_registry`].
    pub(super) fn registry(&self) -> Option<Opaque<Value>> {
        self.registry
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
}

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
