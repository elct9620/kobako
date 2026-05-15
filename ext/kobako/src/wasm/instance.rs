//! `Kobako::Wasm::Instance` — the only Ruby-visible wasmtime wrapper.
//!
//! Constructed via [`Instance::from_path`]; the wasmtime [`Engine`] and
//! compiled [`Module`] are owned by the [`super::cache`] singletons and
//! never surface to Ruby. The instance wraps a [`StoreCell`] (interior-
//! mutability around `wasmtime::Store<HostState>`) plus two cached
//! [`TypedFunc`] handles for the SPEC ABI exports used by the host-driven
//! run path.
//!
//! The Ruby surface intentionally exposes intent, not the underlying ABI
//! (SPEC.md "Code Organization"). The two-frame stdin protocol, packed-u64
//! outcome encoding, and `__kobako_alloc` / `__kobako_take_outcome` /
//! `__kobako_run` exports are all wrapped inside [`Instance::run`] and
//! [`Instance::outcome`]; Ruby callers see only `#run(preamble, source)`,
//! `#stdout`, `#stderr`, `#outcome!`, and `#registry=`.
//!
//! WASI stdout/stderr capture (SPEC.md B-04): wasmtime-wasi p1 bindings
//! route guest fd 1 and fd 2 into per-run [`MemoryOutputPipe`] instances
//! rebuilt at the start of every [`Instance::run`]. The per-channel cap
//! is enforced directly on the pipe — the pipe is sized at `cap + 1` so
//! a guest that writes exactly `cap` bytes is distinguishable from one
//! that exceeded the cap, and `#stdout` / `#stderr` slice the captured
//! bytes back to `cap` before returning them paired with a truncation
//! flag. Uncapped channels (`None`) build the pipe at `usize::MAX`;
//! `memory_limit` provides the real upper bound in that case.
//!
//! Per-run cap enforcement (SPEC.md B-01, E-19, E-20): every Store
//! installs an epoch-deadline callback for wall-clock timeout and a
//! [`ResourceLimiter`] for the linear-memory cap. Wasmtime turns
//! limiter / callback errors into traps; `Instance::run` downcasts the
//! trap source to surface as `Kobako::Wasm::TimeoutError` or
//! `Kobako::Wasm::MemoryLimitError` so the `Sandbox` layer can map them
//! to the named `Kobako::TrapError` subclasses.
//!
//! [`Engine`]: wasmtime::Engine
//! [`Module`]: wasmtime::Module
//! [`TypedFunc`]: wasmtime::TypedFunc
//! [`MemoryOutputPipe`]: wasmtime_wasi::p2::pipe::MemoryOutputPipe
//! [`ResourceLimiter`]: wasmtime::ResourceLimiter

use std::path::Path;
use std::time::{Duration, Instant};

use magnus::{value::Opaque, Error as MagnusError, RArray, RString, Ruby, Value};
use wasmtime::{
    AsContextMut, Caller, Extern, Instance as WtInstance, Linker, Memory, Module as WtModule,
    ResourceLimiter, Store as WtStore, StoreContextMut, TypedFunc, UpdateDeadline,
};
use wasmtime_wasi::p1;
use wasmtime_wasi::p2::pipe::{MemoryInputPipe, MemoryOutputPipe};
use wasmtime_wasi::WasiCtxBuilder;

use super::cache::{cached_module, shared_engine};
use super::dispatch;
use super::host_state::{HostState, MemoryLimitTrap, StoreCell, TimeoutTrap};
use super::{memory_limit_err, timeout_err, wasm_err};

#[magnus::wrap(class = "Kobako::Wasm::Instance", free_immediately, size)]
pub(crate) struct Instance {
    inner: WtInstance,
    store: StoreCell,
    // Cached TypedFunc handles for the two host-driven ABI exports.
    // Optional because test fixtures (a minimal "ping" module) need not
    // provide them; real kobako.wasm always does, and the run-path methods
    // raise a Ruby `Kobako::Wasm::Error` when an export is missing.
    //
    // `__kobako_alloc` is NOT cached here — only `dispatch.rs` calls it,
    // and it does so through `Caller::get_export` on the wasmtime side.
    run: Option<TypedFunc<(), ()>>,
    take_outcome: Option<TypedFunc<(), u64>>,
    // Wall-clock cap for one guest `#run` (SPEC.md B-01); `None` disables
    // the cap. Translated into an `Instant`-based deadline stamped into
    // [`HostState`] at the top of every `Instance::run`.
    timeout: Option<Duration>,
    // Per-channel byte caps for guest stdout / stderr capture (SPEC.md
    // B-01 / B-04). `None` disables the cap on that channel. Read by
    // [`Instance::refresh_wasi`] to size the MemoryOutputPipe and by
    // [`Instance::stdout`] / [`Instance::stderr`] to compute the
    // truncation flag. See the module-level note above for the `cap + 1`
    // sizing rationale. Unlike `memory_limit` (which lives on
    // [`HostState`] because the wasmtime [`ResourceLimiter`] callback
    // consumes it from within the wasm engine), these caps are read only
    // by Instance methods, so they live on Instance itself.
    stdout_limit_bytes: Option<usize>,
    stderr_limit_bytes: Option<usize>,
}

impl Instance {
    /// Construct an Instance from a wasm file path, using the process-wide
    /// shared Engine and per-path Module cache. The single Ruby-facing
    /// constructor for `Kobako::Wasm::Instance` — Engine and Module are
    /// never visible to Ruby.
    ///
    /// `timeout_seconds` is the SPEC.md B-01 wall-clock cap in seconds
    /// (`None` disables); `memory_limit` is the linear-memory cap in
    /// bytes (`None` disables); `stdout_limit_bytes` / `stderr_limit_bytes`
    /// are the per-channel output caps (SPEC.md B-01 / B-04; `None`
    /// disables). All four are validated by the caller
    /// (`Kobako::Sandbox`); this method only refuses non-finite or
    /// non-positive timeouts as a defence in depth.
    pub(crate) fn from_path(
        path: String,
        timeout_seconds: Option<f64>,
        memory_limit: Option<usize>,
        stdout_limit_bytes: Option<usize>,
        stderr_limit_bytes: Option<usize>,
    ) -> Result<Self, MagnusError> {
        let ruby = Ruby::get().expect("Ruby thread");
        let timeout = match timeout_seconds {
            None => None,
            Some(secs) if secs.is_finite() && secs > 0.0 => Some(Duration::from_secs_f64(secs)),
            Some(secs) => {
                return Err(wasm_err(
                    &ruby,
                    format!("timeout_seconds must be > 0 and finite, got {secs}"),
                ));
            }
        };

        let engine = shared_engine()?;
        let module = cached_module(Path::new(&path))?;

        let mut store = WtStore::new(engine, HostState::new(memory_limit));
        store.limiter(|state: &mut HostState| -> &mut dyn ResourceLimiter { state.limiter_mut() });
        store.epoch_deadline_callback(epoch_deadline_callback);

        let store_cell = StoreCell::new(store);
        Self::build(
            engine,
            &module,
            store_cell,
            timeout,
            stdout_limit_bytes,
            stderr_limit_bytes,
        )
    }

    /// Build an `Instance` from an engine, module, and store cell. The
    /// store cell is moved in and ends up owned by the returned Instance.
    /// Wires the WASI p1 imports plus the `__kobako_dispatch` host import.
    fn build(
        engine: &wasmtime::Engine,
        module: &WtModule,
        store_cell: StoreCell,
        timeout: Option<Duration>,
        stdout_limit_bytes: Option<usize>,
        stderr_limit_bytes: Option<usize>,
    ) -> Result<Self, MagnusError> {
        let ruby = Ruby::get().expect("Ruby thread");
        let mut linker: Linker<HostState> = Linker::new(engine);

        // Wire the wasmtime-wasi preview1 WASI imports. Routes guest fd 1/2
        // to the MemoryOutputPipes set up before each run via
        // `Instance::run`. The closure pulls a `&mut WasiP1Ctx` out of
        // HostState; the panic semantics live inside `HostState::wasi_mut`
        // so the wiring stays honest about its precondition.
        p1::add_to_linker_sync(&mut linker, |state: &mut HostState| state.wasi_mut())
            .map_err(|e| wasm_err(&ruby, format!("add WASI p1 to linker: {}", e)))?;

        // `__kobako_dispatch` host import. Signature per SPEC Wire ABI:
        //   (req_ptr: i32, req_len: i32) -> i64
        // Decodes the Request bytes, dispatches via the Ruby-side
        // `Kobako::Registry` (set per-run via `set_registry`), allocates a
        // guest buffer through `__kobako_alloc`, writes the Response bytes
        // there, and returns the packed `(ptr<<32)|len`. The dispatcher
        // returns 0 on any wire-layer fault (including a missing
        // Registry); see `dispatch::handle`.
        linker
            .func_wrap(
                "env",
                "__kobako_dispatch",
                |mut caller: Caller<'_, HostState>, req_ptr: i32, req_len: i32| -> i64 {
                    dispatch::handle(&mut caller, req_ptr, req_len)
                },
            )
            .map_err(|e| wasm_err(&ruby, format!("define __kobako_dispatch: {}", e)))?;

        let instance = {
            let mut store_ref = store_cell.borrow_mut();
            linker
                .instantiate(store_ref.as_context_mut(), module)
                .map_err(|e| instantiate_err(&ruby, e))?
        };

        // Best-effort export lookup. Missing exports are not an error here
        // (test fixture is a bare module); the host enforces presence at
        // invocation time by raising a Ruby `Kobako::Wasm::Error` when the
        // cached Option is None. Only the SPEC ABI `() -> ()` shape is
        // accepted for `__kobako_run`.
        let (run, take_outcome) = {
            let mut store_ref = store_cell.borrow_mut();
            let mut ctx = store_ref.as_context_mut();
            let run = instance
                .get_typed_func::<(), ()>(&mut ctx, "__kobako_run")
                .ok();
            let take_outcome = instance
                .get_typed_func::<(), u64>(&mut ctx, "__kobako_take_outcome")
                .ok();
            (run, take_outcome)
        };

        Ok(Self {
            inner: instance,
            store: store_cell,
            run,
            take_outcome,
            timeout,
            stdout_limit_bytes,
            stderr_limit_bytes,
        })
    }

    /// Install the Ruby-side `Kobako::Registry` into HostState. Bound to
    /// Ruby as `Instance#registry=`. From this point on, every
    /// `__kobako_dispatch` import invocation routes through
    /// `registry.dispatch(req_bytes)`.
    pub(crate) fn set_registry(&self, registry: Value) -> Result<(), MagnusError> {
        let mut store_ref = self.store.borrow_mut();
        store_ref.data_mut().bind_registry(Opaque::from(registry));
        Ok(())
    }

    // -----------------------------------------------------------------
    // Run-path methods. Each method is best-effort — it raises a Ruby
    // `Kobako::Wasm::Error` when the corresponding export is missing or
    // fails so the Sandbox layer can map errors to the three-class
    // taxonomy.
    // -----------------------------------------------------------------

    /// Execute one guest run.
    ///
    /// Rebuilds the WASI context with fresh stdin / stdout / stderr pipes
    /// (the two-frame stdin protocol carries +preamble+ then +source+ —
    /// SPEC.md ABI Signatures), then invokes `__kobako_run`. Per-run
    /// caps (SPEC.md B-01) are primed here: the wall-clock deadline is
    /// stamped into [`HostState`] and the epoch deadline is set to fire
    /// at the next ticker tick; the memory-cap limiter is already wired.
    pub(crate) fn run(&self, preamble: RString, source: RString) -> Result<(), MagnusError> {
        let ruby = Ruby::get().expect("Ruby thread");
        let run = self
            .run
            .as_ref()
            .ok_or_else(|| wasm_err(&ruby, "guest does not export __kobako_run"))?;
        self.refresh_wasi(preamble, source)?;
        self.prime_caps();
        let result = self.call_guest(run);
        self.disarm_caps();
        result.map_err(|e| run_call_err(&ruby, e))
    }

    /// Return the stdout capture from the most recent run as a Ruby
    /// `[bytes, truncated]` Array — `bytes` is a binary String containing
    /// the captured prefix (clipped to `stdout_limit_bytes` when set),
    /// and `truncated` is a boolean that is `true` only when the guest
    /// wrote strictly more than the cap. The pair is recomputed from the
    /// underlying pipe contents on every call; the pipe itself is not
    /// drained until the next `#run` rebuilds it.
    pub(crate) fn stdout(&self) -> Result<RArray, MagnusError> {
        let ruby = Ruby::get().expect("Ruby thread");
        let raw = self.store.borrow().data().stdout_bytes();
        capture_pair(&ruby, &raw, self.stdout_limit_bytes)
    }

    /// Return the stderr capture from the most recent run. Same shape
    /// and semantics as [`Instance::stdout`].
    pub(crate) fn stderr(&self) -> Result<RArray, MagnusError> {
        let ruby = Ruby::get().expect("Ruby thread");
        let raw = self.store.borrow().data().stderr_bytes();
        capture_pair(&ruby, &raw, self.stderr_limit_bytes)
    }

    /// Read OUTCOME_BUFFER bytes captured during the most recent run.
    /// Bound to Ruby as `Instance#outcome!`. The bang signals that the
    /// underlying `__kobako_take_outcome` export is guest-side destructive
    /// — the buffer pointer is invalidated after this call, so a second
    /// invocation within the same run is undefined — and that any failure
    /// (missing export, length overflow, OOB read) raises
    /// `Kobako::Wasm::Error`.
    pub(crate) fn outcome(&self) -> Result<RString, MagnusError> {
        let ruby = Ruby::get().expect("Ruby thread");
        let bytes = self.fetch_outcome_bytes(&ruby)?;
        Ok(ruby.str_from_slice(&bytes))
    }

    // -----------------------------------------------------------------
    // Private helpers.
    // -----------------------------------------------------------------

    /// Stamp the per-run wall-clock deadline into [`HostState`] and prime
    /// the wasmtime epoch deadline so the next ticker tick wakes the
    /// epoch-deadline callback. When `timeout` is disabled, the deadline
    /// is set far enough in the future that the callback effectively
    /// never fires.
    fn prime_caps(&self) {
        let mut store_ref = self.store.borrow_mut();
        match self.timeout {
            Some(timeout) => {
                let deadline = Instant::now() + timeout;
                store_ref.data_mut().set_deadline(Some(deadline));
                store_ref.set_epoch_deadline(1);
            }
            None => {
                store_ref.data_mut().set_deadline(None);
                store_ref.set_epoch_deadline(u64::MAX);
            }
        }
        store_ref.data_mut().limiter_mut().activate();
    }

    /// Drop the memory cap as soon as the guest call returns so that
    /// any post-run host bookkeeping (e.g. fetching the OUTCOME_BUFFER,
    /// which can grow guest memory transiently) is not attributed to
    /// the user script. Paired with [`Instance::prime_caps`].
    fn disarm_caps(&self) {
        self.store
            .borrow_mut()
            .data_mut()
            .limiter_mut()
            .deactivate();
    }

    /// Invoke the cached `__kobako_run` TypedFunc against the live
    /// Store. Lives in its own helper so [`Instance::run`] reads as
    /// the run-path outline (export check → refresh WASI → prime caps
    /// → call guest → disarm caps → map errors) without the
    /// `RefCell::borrow_mut` boilerplate inline.
    fn call_guest(&self, run: &TypedFunc<(), ()>) -> wasmtime::Result<()> {
        let mut store_ref = self.store.borrow_mut();
        run.call(store_ref.as_context_mut(), ())
    }

    /// Rebuild the WASI context with fresh stdin (two-frame: preamble then
    /// source) plus fresh stdout/stderr pipes. Called at the top of every
    /// `#run`. Each pipe is sized at `cap + 1` so [`Instance::stdout`] /
    /// [`Instance::stderr`] can distinguish "wrote exactly cap bytes"
    /// from "exceeded cap"; uncapped channels fall back to `usize::MAX`
    /// and rely on `memory_limit` (E-20) for the real ceiling.
    fn refresh_wasi(&self, preamble: RString, source: RString) -> Result<(), MagnusError> {
        // SAFETY: `as_slice` borrows are scoped to building the stdin Vec
        // below — no Ruby allocations happen between the borrow and the
        // copy, so the underlying RString cannot move.
        let preamble_bytes: &[u8] = unsafe { preamble.as_slice() };
        let source_bytes: &[u8] = unsafe { source.as_slice() };

        let mut stdin_content: Vec<u8> =
            Vec::with_capacity(4 + preamble_bytes.len() + 4 + source_bytes.len());
        // Frame 1 — preamble
        stdin_content.extend_from_slice(&(preamble_bytes.len() as u32).to_be_bytes());
        stdin_content.extend_from_slice(preamble_bytes);
        // Frame 2 — user script
        stdin_content.extend_from_slice(&(source_bytes.len() as u32).to_be_bytes());
        stdin_content.extend_from_slice(source_bytes);

        let stdin_pipe = MemoryInputPipe::new(stdin_content);
        let stdout_pipe = MemoryOutputPipe::new(pipe_capacity(self.stdout_limit_bytes));
        let stderr_pipe = MemoryOutputPipe::new(pipe_capacity(self.stderr_limit_bytes));

        let mut builder = WasiCtxBuilder::new();
        builder.stdin(stdin_pipe);
        builder.stdout(stdout_pipe.clone());
        builder.stderr(stderr_pipe.clone());
        let wasi = builder.build_p1();

        self.store
            .borrow_mut()
            .data_mut()
            .install_wasi(wasi, stdout_pipe, stderr_pipe);

        Ok(())
    }

    /// Invoke `__kobako_take_outcome`, decode the packed +(ptr<<32)|len+
    /// u64, and copy the OUTCOME_BUFFER slice out of guest memory. Raises
    /// `Kobako::Wasm::Error` when the export is missing, the +ptr+/+len+
    /// arithmetic overflows, the slice falls outside live memory, or the
    /// `memory` export itself is absent.
    fn fetch_outcome_bytes(&self, ruby: &Ruby) -> Result<Vec<u8>, MagnusError> {
        let take = self
            .take_outcome
            .as_ref()
            .ok_or_else(|| wasm_err(ruby, "guest does not export __kobako_take_outcome"))?;

        let mut store_ref = self.store.borrow_mut();
        let packed = take
            .call(store_ref.as_context_mut(), ())
            .map_err(|e| wasm_err(ruby, format!("__kobako_take_outcome(): {}", e)))?;
        let ptr = ((packed >> 32) & 0xffff_ffff) as usize;
        let len = (packed & 0xffff_ffff) as usize;

        let mem: Memory = match self.inner.get_export(store_ref.as_context_mut(), "memory") {
            Some(Extern::Memory(m)) => m,
            _ => return Err(wasm_err(ruby, "guest does not export 'memory'")),
        };
        let data = mem.data(store_ref.as_context_mut());
        let end = ptr
            .checked_add(len)
            .ok_or_else(|| wasm_err(ruby, "outcome: ptr + len overflow"))?;
        if end > data.len() {
            return Err(wasm_err(
                ruby,
                format!(
                    "outcome: range [{}, {}) exceeds memory size {}",
                    ptr,
                    end,
                    data.len()
                ),
            ));
        }
        Ok(data[ptr..end].to_vec())
    }
}

/// Translate a per-channel byte cap into the MemoryOutputPipe capacity:
/// `cap + 1` (saturated against `usize::MAX`) when a cap is set so the
/// "wrote exactly cap" and "exceeded cap" cases stay distinguishable;
/// `usize::MAX` when the channel is uncapped.
fn pipe_capacity(cap: Option<usize>) -> usize {
    match cap {
        Some(c) => c.saturating_add(1),
        None => usize::MAX,
    }
}

/// Pure slicing core shared by [`Instance::stdout`] / [`Instance::stderr`]:
/// given the unclipped pipe snapshot and the configured cap, return the
/// bytes Ruby should observe (clipped to `cap`) plus the truncation flag.
/// `truncated` is `true` only when the snapshot strictly exceeded the cap
/// — this is the "wrote `cap + 1` bytes into a `cap + 1`-sized pipe" case;
/// "wrote exactly `cap` bytes" stays `false`.
fn clip_capture(raw: &[u8], cap: Option<usize>) -> (&[u8], bool) {
    match cap {
        Some(c) if raw.len() > c => (&raw[..c], true),
        _ => (raw, false),
    }
}

/// Build the `[bytes, truncated]` Ruby Array surfaced by
/// [`Instance::stdout`] / [`Instance::stderr`]. Delegates the slicing
/// to [`clip_capture`] so the channel-agnostic logic stays unit-
/// testable from `cargo test`.
fn capture_pair(ruby: &Ruby, raw: &[u8], cap: Option<usize>) -> Result<RArray, MagnusError> {
    let (visible, truncated) = clip_capture(raw, cap);
    let arr = ruby.ary_new_capa(2);
    arr.push(ruby.str_from_slice(visible))?;
    arr.push(truncated)?;
    Ok(arr)
}

#[cfg(test)]
mod tests {
    //! Host-side unit tests for the pure capture helpers. The Ruby-
    //! facing E2E suite exercises stdout only (the kobako mrbgem
    //! allowlist excludes guest fd 2 writes); these tests pin the
    //! channel-agnostic slicing so a regression that only breaks one
    //! channel cannot sneak through.
    use super::{clip_capture, pipe_capacity};

    #[test]
    fn pipe_capacity_adds_one_when_cap_is_set() {
        assert_eq!(pipe_capacity(Some(5)), 6);
        assert_eq!(pipe_capacity(Some(0)), 1);
    }

    #[test]
    fn pipe_capacity_falls_back_to_usize_max_when_uncapped() {
        assert_eq!(pipe_capacity(None), usize::MAX);
    }

    #[test]
    fn pipe_capacity_saturates_at_usize_max() {
        assert_eq!(pipe_capacity(Some(usize::MAX)), usize::MAX);
    }

    #[test]
    fn clip_capture_returns_full_bytes_when_under_cap() {
        let (bytes, truncated) = clip_capture(b"abc", Some(5));
        assert_eq!(bytes, b"abc");
        assert!(!truncated);
    }

    #[test]
    fn clip_capture_does_not_flag_truncation_at_exactly_cap_bytes() {
        let (bytes, truncated) = clip_capture(b"abcde", Some(5));
        assert_eq!(bytes, b"abcde");
        assert!(!truncated);
    }

    #[test]
    fn clip_capture_clips_to_cap_and_flags_truncation_on_overflow() {
        // The pipe is sized `cap + 1`, so the snapshot can be at most
        // 6 bytes when `cap == 5`; that surface is what triggers the
        // truncation flag.
        let (bytes, truncated) = clip_capture(b"abcdef", Some(5));
        assert_eq!(bytes, b"abcde");
        assert!(truncated);
    }

    #[test]
    fn clip_capture_treats_none_as_uncapped() {
        let (bytes, truncated) = clip_capture(b"abcdef", None);
        assert_eq!(bytes, b"abcdef");
        assert!(!truncated);
    }

    #[test]
    fn clip_capture_handles_empty_input() {
        let (bytes, truncated) = clip_capture(b"", Some(5));
        assert_eq!(bytes, b"");
        assert!(!truncated);
    }
}

/// Epoch-deadline callback installed on every Store. Read the per-run
/// wall-clock deadline from [`HostState`] (SPEC.md B-01) and trap with
/// [`TimeoutTrap`] once the deadline has passed; otherwise extend the
/// next check by one tick of the process-wide epoch ticker. When the
/// deadline is `None` the callback should not fire under normal
/// `Instance::run` flow because `set_epoch_deadline(u64::MAX)` is used;
/// returning a long extension keeps the callback inert as a defence in
/// depth.
fn epoch_deadline_callback(
    ctx: StoreContextMut<'_, HostState>,
) -> wasmtime::Result<UpdateDeadline> {
    match ctx.data().deadline() {
        Some(deadline) if Instant::now() >= deadline => Err(wasmtime::Error::new(TimeoutTrap)),
        Some(_) => Ok(UpdateDeadline::Continue(1)),
        None => Ok(UpdateDeadline::Continue(u64::MAX / 2)),
    }
}

/// Map a wasmtime call error to the right `Kobako::Wasm::*` Ruby
/// exception class. `__kobako_run` traps are downcast to identify the
/// configured-cap path (SPEC.md E-19 / E-20); everything else surfaces
/// as the base `Kobako::Wasm::Error`.
fn run_call_err(ruby: &Ruby, err: wasmtime::Error) -> MagnusError {
    if err.downcast_ref::<TimeoutTrap>().is_some() {
        return timeout_err(ruby, format!("__kobako_run(): {}", err));
    }
    if err.downcast_ref::<MemoryLimitTrap>().is_some() {
        return memory_limit_err(ruby, format!("__kobako_run(): {}", err));
    }
    wasm_err(ruby, format!("__kobako_run(): {}", err))
}

/// Map an instantiation error to the right `Kobako::Wasm::*` Ruby
/// exception. The memory cap is dormant during instantiation by design
/// (see [`HostState::set_memory_cap_active`]), but [`MemoryLimitTrap`]
/// is still possible if a future Sandbox configuration enables it
/// during instantiation — keep the mapping symmetric with
/// [`run_call_err`].
fn instantiate_err(ruby: &Ruby, err: wasmtime::Error) -> MagnusError {
    if err.downcast_ref::<MemoryLimitTrap>().is_some() {
        return memory_limit_err(ruby, format!("instantiate: {}", err));
    }
    wasm_err(ruby, format!("instantiate: {}", err))
}
