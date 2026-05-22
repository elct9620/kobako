//! `Kobako::Wasm::Instance` — the only Ruby-visible wasmtime wrapper.
//!
//! Constructed via [`Instance::from_path`]; the wasmtime [`Engine`] and
//! compiled [`Module`] are owned by the [`super::cache`] singletons and
//! never surface to Ruby. The instance wraps a [`StoreCell`] (interior-
//! mutability around `wasmtime::Store<HostState>`) plus three cached
//! [`TypedFunc`] handles for the docs/wire-codec.md ABI exports used by
//! the host-driven run path.
//!
//! The Ruby surface intentionally exposes intent, not the underlying ABI
//! (SPEC.md "Code Organization"). The length-prefixed stdin frame
//! protocol (three frames for `#eval`: preamble + source + snippets;
//! two for `#run`: preamble + snippets), packed-u64 outcome encoding,
//! and the `__kobako_eval` / `__kobako_run` / `__kobako_alloc` /
//! `__kobako_take_outcome` exports are all wrapped inside
//! [`Instance::eval`], [`Instance::run`], and [`Instance::outcome`];
//! Ruby callers see only `#eval(preamble, source, snippets)`,
//! `#run(preamble, snippets, envelope)`, `#stdout`, `#stderr`,
//! `#outcome!`, and `#server=`.
//!
//! WASI stdout/stderr capture (docs/behavior.md B-04): wasmtime-wasi p1
//! bindings route guest fd 1 and fd 2 into per-run [`MemoryOutputPipe`]
//! instances rebuilt at the start of every [`Instance::eval`] /
//! [`Instance::run`]. The per-channel cap is enforced directly on the
//! pipe — the pipe is sized at `cap + 1` so a guest that writes exactly
//! `cap` bytes is distinguishable from one that exceeded the cap, and
//! `#stdout` / `#stderr` slice the captured bytes back to `cap` before
//! returning them paired with a truncation flag. Uncapped channels
//! (`None`) build the pipe at `usize::MAX`; `memory_limit` provides
//! the real upper bound in that case.
//!
//! Per-run cap enforcement (docs/behavior.md B-01, E-19, E-20): every
//! Store installs an epoch-deadline callback for wall-clock timeout and
//! a [`ResourceLimiter`] for the linear-memory cap. Wasmtime turns
//! limiter / callback errors into traps; the run-path methods downcast
//! the trap source to surface as `Kobako::Wasm::TimeoutError` or
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
use super::{memory_limit_err, rstring_to_vec, timeout_err, wasm_err};

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
    eval: Option<TypedFunc<(), ()>>,
    run: Option<TypedFunc<(i32, i32), ()>>,
    take_outcome: Option<TypedFunc<(), u64>>,
    // Wall-clock cap for one guest `#run` (docs/behavior.md B-01); `None` disables
    // the cap. Translated into an `Instant`-based deadline stamped into
    // [`HostState`] at the top of every `Instance::eval`.
    timeout: Option<Duration>,
    // Per-channel byte caps for guest stdout / stderr capture
    // (docs/behavior.md B-01 / B-04). `None` disables the cap on that
    // channel. Read by
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
    /// `timeout_seconds` is the docs/behavior.md B-01 wall-clock cap in seconds
    /// (`None` disables); `memory_limit` is the linear-memory cap in
    /// bytes (`None` disables); `stdout_limit_bytes` / `stderr_limit_bytes`
    /// are the per-channel output caps (docs/behavior.md B-01 / B-04; `None`
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
                    format!("timeout must be > 0 and finite, got {secs} seconds"),
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
        // `Instance::eval`. The closure pulls a `&mut WasiP1Ctx` out of
        // HostState; the panic semantics live inside `HostState::wasi_mut`
        // so the wiring stays honest about its precondition.
        p1::add_to_linker_sync(&mut linker, |state: &mut HostState| state.wasi_mut()).map_err(
            |e| {
                wasm_err(
                    &ruby,
                    format!("failed to wire WASI runtime into Sandbox: {}", e),
                )
            },
        )?;

        // `__kobako_dispatch` host import. Signature per SPEC Wire ABI:
        //   (req_ptr: i32, req_len: i32) -> i64
        // Decodes the Request bytes, dispatches via the Ruby-side
        // `Kobako::RPC::Server` (set per-run via `set_server`), allocates a
        // guest buffer through `__kobako_alloc`, writes the Response bytes
        // there, and returns the packed `(ptr<<32)|len`. The dispatcher
        // returns 0 on any wire-layer fault (including a missing
        // Server); see `dispatch::handle`.
        linker
            .func_wrap(
                "env",
                "__kobako_dispatch",
                |mut caller: Caller<'_, HostState>, req_ptr: i32, req_len: i32| -> i64 {
                    dispatch::handle(&mut caller, req_ptr, req_len)
                },
            )
            .map_err(|e| {
                wasm_err(
                    &ruby,
                    format!("failed to register host RPC dispatch import: {}", e),
                )
            })?;

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
        // accepted for `__kobako_eval`; `__kobako_run` takes
        // `(env_ptr, env_len) -> ()` per docs/wire-codec.md § ABI
        // Signatures.
        let (eval, run, take_outcome) = {
            let mut store_ref = store_cell.borrow_mut();
            let mut ctx = store_ref.as_context_mut();
            let eval = instance
                .get_typed_func::<(), ()>(&mut ctx, "__kobako_eval")
                .ok();
            let run = instance
                .get_typed_func::<(i32, i32), ()>(&mut ctx, "__kobako_run")
                .ok();
            let take_outcome = instance
                .get_typed_func::<(), u64>(&mut ctx, "__kobako_take_outcome")
                .ok();
            (eval, run, take_outcome)
        };

        Ok(Self {
            inner: instance,
            store: store_cell,
            eval,
            run,
            take_outcome,
            timeout,
            stdout_limit_bytes,
            stderr_limit_bytes,
        })
    }

    /// Install the Ruby-side `Kobako::RPC::Server` into HostState. Bound to
    /// Ruby as `Instance#server=`. From this point on, every
    /// `__kobako_dispatch` import invocation routes through
    /// `server.dispatch(req_bytes)`.
    pub(crate) fn set_server(&self, server: Value) -> Result<(), MagnusError> {
        let mut store_ref = self.store.borrow_mut();
        store_ref.data_mut().bind_server(Opaque::from(server));
        Ok(())
    }

    // -----------------------------------------------------------------
    // Run-path methods. Each method is best-effort — it raises a Ruby
    // `Kobako::Wasm::Error` when the corresponding export is missing or
    // fails so the Sandbox layer can map errors to the three-class
    // taxonomy.
    // -----------------------------------------------------------------

    /// Execute one guest invocation (`__kobako_eval` — one-shot source).
    ///
    /// Rebuilds the WASI context with fresh stdin / stdout / stderr pipes
    /// (the three-frame stdin protocol carries +preamble+, +source+, then
    /// +snippets+ — docs/wire-codec.md § Invocation channels), then
    /// invokes `__kobako_eval`. Per-invocation caps (docs/behavior.md
    /// B-01) are primed here: the wall-clock deadline is stamped into
    /// [`HostState`] and the epoch deadline is set to fire at the next
    /// ticker tick; the memory-cap limiter is already wired.
    pub(crate) fn eval(
        &self,
        preamble: RString,
        source: RString,
        snippets: RString,
    ) -> Result<(), MagnusError> {
        let ruby = Ruby::get().expect("Ruby thread");
        let eval = require_export(&ruby, self.eval.as_ref(), "__kobako_eval")?;
        self.refresh_wasi(&[
            rstring_to_vec(preamble),
            rstring_to_vec(source),
            rstring_to_vec(snippets),
        ]);
        self.prime_caps();
        let result = {
            let mut store_ref = self.store.borrow_mut();
            eval.call(store_ref.as_context_mut(), ())
        };
        self.disarm_caps();
        result.map_err(|e| call_err(&ruby, e))
    }

    /// Execute one entrypoint dispatch (`__kobako_run`).
    ///
    /// Rebuilds the WASI context with the two-frame stdin protocol
    /// (preamble + snippets; no user source frame — docs/wire-codec.md
    /// § Invocation channels), copies +envelope+ bytes into guest linear
    /// memory via `__kobako_alloc`, and calls `__kobako_run(env_ptr,
    /// env_len)`. Per-invocation cap semantics match [`Instance::eval`].
    /// Returns +Kobako::Wasm::Error+ ("alloc returned 0") when guest
    /// allocation fails (docs/behavior.md E-31).
    pub(crate) fn run(
        &self,
        preamble: RString,
        snippets: RString,
        envelope: RString,
    ) -> Result<(), MagnusError> {
        let ruby = Ruby::get().expect("Ruby thread");
        let run = require_export(&ruby, self.run.as_ref(), "__kobako_run")?;
        self.refresh_wasi(&[rstring_to_vec(preamble), rstring_to_vec(snippets)]);
        let (env_ptr, env_len) = self.write_envelope(&ruby, envelope)?;
        self.prime_caps();
        let result = {
            let mut store_ref = self.store.borrow_mut();
            run.call(store_ref.as_context_mut(), (env_ptr, env_len))
        };
        self.disarm_caps();
        result.map_err(|e| call_err(&ruby, e))
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

    /// Stamp the per-invocation wall-clock deadline into [`HostState`]
    /// and prime the wasmtime epoch deadline so the next ticker tick
    /// wakes the epoch-deadline callback. When `timeout` is disabled,
    /// the deadline is set far enough in the future that the callback
    /// effectively never fires.
    ///
    /// Also captures the current linear-memory size as the baseline
    /// for the docs/behavior.md E-20 per-invocation memory delta cap.
    /// The mruby image's declared initial allocation and the high-water
    /// mark left by prior invocations on the same Sandbox are folded
    /// into the baseline rather than the budget — only `memory.grow`
    /// past +baseline+ counts against `memory_limit`.
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
        let baseline = match self.inner.get_export(store_ref.as_context_mut(), "memory") {
            Some(Extern::Memory(m)) => m.data_size(store_ref.as_context_mut()),
            _ => 0,
        };
        store_ref.data_mut().arm_memory_cap(baseline);
    }

    /// Drop the memory cap as soon as the guest call returns so that
    /// any post-run host bookkeeping (e.g. fetching the OUTCOME_BUFFER,
    /// which can grow guest memory transiently) is not attributed to
    /// the user script. Paired with [`Instance::prime_caps`].
    fn disarm_caps(&self) {
        self.store.borrow_mut().data_mut().disarm_memory_cap();
    }

    /// Allocate a +len+-byte buffer in guest linear memory via
    /// `__kobako_alloc`, copy +envelope+ into it, and return +(ptr, len)+
    /// as +i32+ values matching the `__kobako_run(env_ptr, env_len)` ABI.
    /// Raises +Kobako::Wasm::Error+ when the guest export is missing or
    /// allocation fails (docs/behavior.md E-31).
    fn write_envelope(&self, ruby: &Ruby, envelope: RString) -> Result<(i32, i32), MagnusError> {
        let bytes = rstring_to_vec(envelope);
        let len_i32 = envelope_len_to_i32(bytes.len()).map_err(|msg| wasm_err(ruby, msg))?;

        let mut store_ref = self.store.borrow_mut();
        let alloc: TypedFunc<u32, u32> = self
            .inner
            .get_typed_func(store_ref.as_context_mut(), "__kobako_alloc")
            .map_err(|_| wasm_err(ruby, GUEST_BINARY_MISSING_RUNTIME))?;
        let ptr = alloc
            .call(store_ref.as_context_mut(), bytes.len() as u32)
            .map_err(|e| {
                wasm_err(
                    ruby,
                    format!("guest failed to allocate input buffer: {}", e),
                )
            })?;
        if ptr == 0 {
            return Err(wasm_err(
                ruby,
                "guest could not allocate input buffer (out of memory)",
            ));
        }

        let memory: Memory = match self.inner.get_export(store_ref.as_context_mut(), "memory") {
            Some(Extern::Memory(m)) => m,
            _ => return Err(wasm_err(ruby, GUEST_BINARY_NOT_KOBAKO)),
        };
        let data = memory.data_mut(store_ref.as_context_mut());
        let range = guest_buffer_range(ptr as usize, bytes.len(), data.len())
            .map_err(|msg| wasm_err(ruby, msg))?;
        data[range].copy_from_slice(&bytes);

        Ok((ptr as i32, len_i32))
    }

    /// Rebuild the WASI context with fresh stdin (carrying every frame in
    /// +frames+, each prefixed by its 4-byte big-endian u32 length —
    /// docs/wire-codec.md § Invocation channels) plus fresh stdout / stderr
    /// pipes. Called at the top of every guest invocation: +#eval+ passes
    /// three frames (preamble, source, snippets), +#run+ passes two
    /// (preamble, snippets — the invocation envelope arrives via linear
    /// memory instead). Each output pipe is sized at `cap + 1` so
    /// [`Instance::stdout`] / [`Instance::stderr`] can distinguish "wrote
    /// exactly cap bytes" from "exceeded cap"; uncapped channels fall back
    /// to `usize::MAX` and rely on `memory_limit` (docs/behavior.md E-20)
    /// for the real ceiling.
    fn refresh_wasi(&self, frames: &[Vec<u8>]) {
        let total: usize = frames.iter().map(|f| 4 + f.len()).sum();
        let mut stdin_content: Vec<u8> = Vec::with_capacity(total);
        for frame in frames {
            stdin_content.extend_from_slice(&(frame.len() as u32).to_be_bytes());
            stdin_content.extend_from_slice(frame);
        }

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
    }

    /// Invoke `__kobako_take_outcome`, decode the packed +(ptr<<32)|len+
    /// u64, and copy the OUTCOME_BUFFER slice out of guest memory. Raises
    /// `Kobako::Wasm::Error` when the export is missing, the +ptr+/+len+
    /// arithmetic overflows, the slice falls outside live memory, or the
    /// `memory` export itself is absent.
    fn fetch_outcome_bytes(&self, ruby: &Ruby) -> Result<Vec<u8>, MagnusError> {
        let take = require_export(ruby, self.take_outcome.as_ref(), "__kobako_take_outcome")?;

        let mut store_ref = self.store.borrow_mut();
        let packed = take
            .call(store_ref.as_context_mut(), ())
            .map_err(|e| wasm_err(ruby, format!("failed to read guest result: {}", e)))?;
        let (ptr, len) = unpack_outcome_packed(packed);

        let mem: Memory = match self.inner.get_export(store_ref.as_context_mut(), "memory") {
            Some(Extern::Memory(m)) => m,
            _ => return Err(wasm_err(ruby, GUEST_BINARY_NOT_KOBAKO)),
        };
        let data = mem.data(store_ref.as_context_mut());
        let range = guest_buffer_range(ptr, len, data.len())
            .map_err(|msg| wasm_err(ruby, format!("guest result is out of bounds: {}", msg)))?;
        Ok(data[range].to_vec())
    }
}

/// User-facing message for the "guest binary is missing one of the
/// internal Kobako runtime exports" failure mode. Phrased in caller
/// vocabulary — the underlying ABI symbol names (`__kobako_alloc`,
/// `__kobako_eval`, `__kobako_take_outcome`) are not actionable for
/// Host App authors; the actionable diagnosis is "your kobako.wasm
/// does not match the host gem version".
const GUEST_BINARY_MISSING_RUNTIME: &str =
    "guest binary is missing required Kobako runtime exports; \
     verify data/kobako.wasm matches the host gem version";

/// User-facing message for the "guest binary is not a kobako-shaped
/// Wasm module at all" failure mode (no linear memory export). Same
/// phrasing philosophy as [`GUEST_BINARY_MISSING_RUNTIME`].
const GUEST_BINARY_NOT_KOBAKO: &str =
    "guest binary does not export linear memory; this is not a kobako-compatible Wasm module";

/// Return the cached +TypedFunc+ for an ABI export, or raise
/// +Kobako::Wasm::Error+ when the option is +None+. The run-path
/// methods (+#eval+, +#run+, +#outcome!+) all share the same
/// "missing export → Ruby error" boilerplate; this helper collapses
/// the three sites onto one safe entry. The +_name+ argument is
/// retained for future operator-side logging but is deliberately not
/// spliced into the user-facing message (see
/// [`GUEST_BINARY_MISSING_RUNTIME`]).
fn require_export<'a, Params, Results>(
    ruby: &Ruby,
    export: Option<&'a TypedFunc<Params, Results>>,
    _name: &str,
) -> Result<&'a TypedFunc<Params, Results>, MagnusError>
where
    Params: wasmtime::WasmParams,
    Results: wasmtime::WasmResults,
{
    export.ok_or_else(|| wasm_err(ruby, GUEST_BINARY_MISSING_RUNTIME))
}

/// Validate the invocation envelope length and return it as +i32+ — the
/// signed wasm wire-ABI parameter type for the guest-run entrypoint.
/// Rejects sizes above +i32::MAX+ (2 GiB) so the downstream cast cannot
/// silently wrap.
fn envelope_len_to_i32(len: usize) -> Result<i32, &'static str> {
    i32::try_from(len).map_err(|_| "invocation payload exceeds 2 GiB")
}

/// Compute the half-open range `[ptr, ptr + len)` for a guest linear-memory
/// copy, validating that the arithmetic does not overflow and the range
/// fits inside `mem_size`. Shared by [`Instance::write_envelope`] (write
/// side) and [`Instance::fetch_outcome_bytes`] (read side).
fn guest_buffer_range(
    ptr: usize,
    len: usize,
    mem_size: usize,
) -> Result<core::ops::Range<usize>, &'static str> {
    let end = ptr.checked_add(len).ok_or("ptr + len overflow")?;
    if end > mem_size {
        return Err("range exceeds guest memory size");
    }
    Ok(ptr..end)
}

/// Unpack the `(ptr, len)` u64 returned by `__kobako_take_outcome`:
/// high 32 bits = ptr, low 32 bits = len. Mirrors the guest-side
/// `crate::abi::unpack_u64` in `wasm/kobako-wasm/src/abi.rs`.
fn unpack_outcome_packed(packed: u64) -> (usize, usize) {
    let ptr = (packed >> 32) as u32 as usize;
    let len = packed as u32 as usize;
    (ptr, len)
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

/// Epoch-deadline callback installed on every Store. Read the per-run
/// wall-clock deadline from [`HostState`] (docs/behavior.md B-01) and trap with
/// [`TimeoutTrap`] once the deadline has passed; otherwise extend the
/// next check by one tick of the process-wide epoch ticker. When the
/// deadline is `None` the callback should not fire under normal
/// `Instance::eval` / `Instance::run` flow because
/// `set_epoch_deadline(u64::MAX)` is used; returning a long extension
/// keeps the callback inert as a defence in depth.
fn epoch_deadline_callback(
    ctx: StoreContextMut<'_, HostState>,
) -> wasmtime::Result<UpdateDeadline> {
    match ctx.data().deadline() {
        Some(deadline) if Instant::now() >= deadline => Err(wasmtime::Error::new(TimeoutTrap)),
        Some(_) => Ok(UpdateDeadline::Continue(1)),
        None => Ok(UpdateDeadline::Continue(u64::MAX / 2)),
    }
}

/// Configured-cap path classification for a wasmtime error. The
/// downcast logic stays in a pure helper so the
/// `Kobako::Wasm::TimeoutError` / `MemoryLimitError` /
/// `Kobako::Wasm::Error` mapping can be exercised from `cargo test`
/// without the magnus surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrapClass {
    /// docs/behavior.md E-19 wall-clock cap path.
    Timeout,
    /// docs/behavior.md E-20 linear-memory cap path.
    MemoryLimit,
    /// Any other wasmtime error — surfaces as the base
    /// `Kobako::Wasm::Error`.
    Other,
}

/// Inspect a wasmtime error to decide which `Kobako::Wasm::*` class it
/// should map to. Pure function — operates on the error's downcast
/// chain only, no magnus / Ruby state required.
fn classify_trap(err: &wasmtime::Error) -> TrapClass {
    if err.downcast_ref::<TimeoutTrap>().is_some() {
        TrapClass::Timeout
    } else if err.downcast_ref::<MemoryLimitTrap>().is_some() {
        TrapClass::MemoryLimit
    } else {
        TrapClass::Other
    }
}

/// Map a wasmtime call error to the right `Kobako::Wasm::*` Ruby
/// exception class. The ABI export symbol (`__kobako_eval` /
/// `__kobako_run`) is deliberately omitted from the message — the
/// Sandbox layer attaches the user-facing verb (`Sandbox#eval` /
/// `Sandbox#run`) so the message reads in caller vocabulary rather
/// than ABI vocabulary.
fn call_err(ruby: &Ruby, err: wasmtime::Error) -> MagnusError {
    let msg = format!("{}", err);
    match classify_trap(&err) {
        TrapClass::Timeout => timeout_err(ruby, msg),
        TrapClass::MemoryLimit => memory_limit_err(ruby, msg),
        TrapClass::Other => wasm_err(ruby, msg),
    }
}

/// Map an instantiation error to the right `Kobako::Wasm::*` Ruby
/// exception. The memory cap is dormant during instantiation by design
/// (see [`HostState::arm_memory_cap`] / [`HostState::disarm_memory_cap`]),
/// but [`MemoryLimitTrap`] is still possible if a future Sandbox
/// configuration enables it during instantiation — keep the mapping
/// symmetric with [`call_err`]. [`TrapClass::Timeout`] is unreachable on
/// the instantiation path (the epoch deadline is not armed yet) but
/// folding it into the same `match` keeps the two paths visually paired.
fn instantiate_err(ruby: &Ruby, err: wasmtime::Error) -> MagnusError {
    let msg = format!("instantiate: {}", err);
    match classify_trap(&err) {
        TrapClass::MemoryLimit => memory_limit_err(ruby, msg),
        TrapClass::Timeout | TrapClass::Other => wasm_err(ruby, msg),
    }
}

#[cfg(test)]
mod tests {
    //! Host-side unit tests for the pure capture helpers. The Ruby-
    //! facing E2E suite exercises stdout only (the kobako mrbgem
    //! allowlist excludes guest fd 2 writes); these tests pin the
    //! channel-agnostic slicing so a regression that only breaks one
    //! channel cannot sneak through.
    use super::{
        classify_trap, clip_capture, envelope_len_to_i32, guest_buffer_range, pipe_capacity,
        unpack_outcome_packed, TrapClass,
    };
    use super::{MemoryLimitTrap, TimeoutTrap};

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

    #[test]
    fn envelope_len_to_i32_accepts_zero_and_max() {
        assert_eq!(envelope_len_to_i32(0), Ok(0));
        assert_eq!(envelope_len_to_i32(i32::MAX as usize), Ok(i32::MAX));
    }

    #[test]
    fn envelope_len_to_i32_rejects_past_i32_max() {
        assert!(envelope_len_to_i32(i32::MAX as usize + 1).is_err());
        assert!(envelope_len_to_i32(usize::MAX).is_err());
    }

    #[test]
    fn guest_buffer_range_returns_half_open_range() {
        // Standard case: ptr + len fits inside memory.
        assert_eq!(guest_buffer_range(10, 5, 100), Ok(10..15));
    }

    #[test]
    fn guest_buffer_range_accepts_zero_length_at_any_in_bounds_ptr() {
        // Zero-length writes / reads must succeed as long as ptr is in
        // bounds — both reactor calls hand zero-length frames through
        // (e.g. an empty Frame 3 snippets list).
        assert_eq!(guest_buffer_range(0, 0, 0), Ok(0..0));
        assert_eq!(guest_buffer_range(42, 0, 100), Ok(42..42));
    }

    #[test]
    fn guest_buffer_range_rejects_ptr_plus_len_overflow() {
        assert!(guest_buffer_range(usize::MAX, 1, usize::MAX).is_err());
    }

    #[test]
    fn guest_buffer_range_rejects_end_past_memory() {
        assert!(guest_buffer_range(10, 100, 50).is_err());
        // End exactly equal to mem_size is in-bounds.
        assert_eq!(guest_buffer_range(0, 50, 50), Ok(0..50));
    }

    #[test]
    fn unpack_outcome_packed_extracts_high_ptr_low_len() {
        assert_eq!(
            unpack_outcome_packed(0xAABB_CCDD_1122_3344),
            (0xAABB_CCDD, 0x1122_3344)
        );
    }

    #[test]
    fn unpack_outcome_packed_zero_decodes_to_zero_pair() {
        assert_eq!(unpack_outcome_packed(0), (0, 0));
    }

    #[test]
    fn classify_trap_routes_timeout_trap_to_timeout() {
        let err = wasmtime::Error::new(TimeoutTrap);
        assert_eq!(classify_trap(&err), TrapClass::Timeout);
    }

    #[test]
    fn classify_trap_routes_memory_limit_trap_to_memory_limit() {
        let err = wasmtime::Error::new(MemoryLimitTrap::new(1 << 20, 1 << 19));
        assert_eq!(classify_trap(&err), TrapClass::MemoryLimit);
    }

    #[test]
    fn classify_trap_falls_back_to_other_for_unknown_errors() {
        let err = wasmtime::Error::msg("some other wasmtime fault");
        assert_eq!(classify_trap(&err), TrapClass::Other);
    }
}
