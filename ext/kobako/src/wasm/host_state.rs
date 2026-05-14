//! Per-Store host state shared with every wasmtime callback.
//!
//! Owned by [`StoreCell`] (a `RefCell` shim wrapping `wasmtime::Store`)
//! and threaded through every host import — the `__kobako_dispatch`
//! dispatcher reads `registry`, while the run-path methods on
//! [`crate::wasm::Instance`] mutate `wasi`, `stdout_pipe`, `stderr_pipe`
//! when refreshing the WASI context before each `#run` (SPEC.md B-03 /
//! B-04).

use std::cell::RefCell;

use magnus::{value::Opaque, Value};
use wasmtime::Store as WtStore;
use wasmtime_wasi::p1::WasiP1Ctx;
use wasmtime_wasi::p2::pipe::MemoryOutputPipe;

/// Per-Store host state threaded through every host import callback.
///
/// WASI p1 state is embedded as `Option<WasiP1Ctx>` so it can be replaced
/// fresh before each `#run` without rebuilding the Store. The `stdout_pipe`
/// and `stderr_pipe` clones are kept alongside so the Ruby layer can read
/// captured bytes after execution without touching the WASI internals.
#[derive(Default)]
pub(crate) struct HostState {
    /// WASI p1 context for the current (or most-recent) run. Replaced before
    /// each `#run` so stdin/stdout/stderr pipes are always fresh (SPEC.md B-03).
    pub wasi: Option<WasiP1Ctx>,
    /// Clone of the MemoryOutputPipe wired to guest fd 1 (stdout). Retained
    /// here so the Ruby `#stdout` reader can call `contents()` after execution
    /// without having to dig into the WASI ctx internals.
    pub stdout_pipe: Option<MemoryOutputPipe>,
    /// Clone of the MemoryOutputPipe wired to guest fd 2 (stderr).
    pub stderr_pipe: Option<MemoryOutputPipe>,
    /// Cached OUTCOME_BUFFER bytes from the most-recent `#run`. Populated on
    /// the first `#outcome!` call (which invokes `__kobako_take_outcome` —
    /// guest-side destructive) and reused on subsequent calls within the same
    /// run, so the Ruby reader is idempotent. Cleared at the start of every
    /// new `#run`.
    pub outcome_cache: Option<Vec<u8>>,
    /// Ruby-side `Kobako::Registry`. When set, the `__kobako_dispatch`
    /// import calls `registry.dispatch(req_bytes)` and hands the returned
    /// Response bytes back to the guest. `Opaque<Value>` is `Send + Sync`;
    /// calling `get_inner` requires a `Ruby` handle, which we obtain on
    /// every Ruby thread entry via `Ruby::get()`.
    pub registry: Option<Opaque<Value>>,
}

/// Interior-mutability wrapper around `wasmtime::Store<HostState>`.
///
/// Magnus requires `Send + Sync` for wrapped types. `wasmtime::Store` is not
/// `Sync`, so we wrap it in a `RefCell`. `RefCell` alone is sufficient
/// because magnus enforces single-threaded GVL access from Ruby; `Send` and
/// `Sync` are asserted via the unsafe impls below.
pub(crate) struct StoreCell(pub(crate) RefCell<WtStore<HostState>>);

// SAFETY: Ruby's GVL serialises access to magnus-wrapped objects on a single
// OS thread at a time. `wasmtime::Store` is `Send` (verified upstream); the
// `RefCell`-mediated mutation is therefore safe under the GVL invariant.
unsafe impl Send for StoreCell {}
unsafe impl Sync for StoreCell {}
