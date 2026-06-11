// Host-side wasmtime runtime wrapper.
//
// The only Ruby-visible class is
//
//   Kobako::Runtime — wraps wasmtime::Instance + cached TypedFuncs
//
// constructed via `Kobako::Runtime.from_path(path, timeout, memory_limit,
// stdout_limit, stderr_limit)`.
// The underlying wasmtime Engine and compiled Module live in a process-scope
// cache (see the `cache` submodule) and never surface to Ruby (SPEC.md "Code
// Organization": `ext/` "exposes no Wasm engine types to the Host App or
// downstream gems").
//
// Module layout (per CLAUDE.md principle #2 — one responsibility per file):
//
//   * `cache`       — process-wide Engine + per-path Module cache and the
//                     process-singleton epoch ticker thread.
//   * `config`      — per-Runtime caps (timeout / stdout / stderr limits).
//   * `exports`     — cached `__kobako_eval` / `_run` / `_take_outcome` handles.
//   * `invocation`  — Invocation (per-Store context), StoreCell wrapper, the
//                     `MemoryLimiter` memory cap, and the trap marker
//                     types (`TimeoutTrap` / `MemoryLimitTrap`).
//   * `dispatch`    — `__kobako_dispatch` host-import dispatch helpers.
//   * `guest_mem`   — Caller-based guest linear-memory alloc / write / read.
//   * `capture`     — stdout / stderr pipe sizing + clip helpers.
//   * `trap`        — wasmtime-error → `Kobako::*` trap classification.
//
// This file owns the `Kobako::Runtime` magnus class itself (the wasmtime
// instance + Store + cached `Exports` + `Config`, plus the `#eval` /
// `#run` run path), the Ruby error-class lazy-resolvers, the `trap_err` /
// `timeout_err` / `memory_limit_err` / `setup_err` constructors shared by
// every submodule, and the Ruby init() that registers the class.

mod ambient;
mod cache;
mod capture;
mod config;
mod dispatch;
mod exports;
mod guest_mem;
mod invocation;
mod trap;

use magnus::value::Lazy;
use magnus::{
    function, method, prelude::*, Error as MagnusError, ExceptionClass, RModule, RString, Ruby,
};

use std::cell::Cell;
use std::path::Path;
use std::time::{Duration, Instant};

use magnus::{gc, typed_data::DataTypeFunctions, value::Opaque, RArray, TypedData, Value};

use crate::snapshot::Snapshot;
use wasmtime::{
    AsContextMut, Caller, Extern, Instance as WtInstance, Linker, Memory, Module as WtModule,
    ResourceLimiter, Store as WtStore, TypedFunc,
};
use wasmtime_wasi::p1;
use wasmtime_wasi::p2::pipe::{MemoryInputPipe, MemoryOutputPipe};
use wasmtime_wasi::WasiCtxBuilder;

use self::cache::{cached_module, shared_engine};
use self::config::Config;
use self::exports::Exports;
use self::invocation::{Invocation, StoreCell};

/// The wire ABI version this host implements (docs/wire-codec.md § ABI
/// Version). A Guest Binary is accepted only when its
/// `__kobako_abi_version` export reports the same value (B-40 / E-42);
/// the guest-side mirror is `kobako_core::abi::ABI_VERSION`.
const ABI_VERSION: u32 = 1;

/// Copy the bytes of `s` into a fresh `Vec<u8>`. Single safe entry to
/// what would otherwise be an inline `unsafe { rstring.as_slice() }
/// .to_vec()` duplicated at every host-↔-guest boundary. The borrow
/// does not outlive this call, so no Ruby allocation can move the
/// underlying RString between the borrow and the copy — the safety
/// invariant the inline form relied on is established once here.
pub(crate) fn rstring_to_vec(s: RString) -> Vec<u8> {
    // SAFETY: see item doc.
    unsafe { s.as_slice() }.to_vec()
}

// ---------------------------------------------------------------------------
// Error classes (lazy-resolved from Ruby once the top-level Kobako error
// hierarchy is loaded by `lib/kobako/errors.rb`). The ext raises directly
// into the invocation-outcome taxonomy (`TrapError` and its subclasses)
// for run-path failures and into the construction-layer `SetupError`
// (and its `ModuleNotBuiltError` subclass) for `from_path` setup failures
// — no engine-specific intermediate layer; the Sandbox layer adds the
// verb prefix and lets the subclass identity flow through unchanged.
// ---------------------------------------------------------------------------

/// Resolve `Kobako::<name>` as an `ExceptionClass` — the shared body of
/// every error-class `Lazy` below, which differ only in the constant
/// name. The constants are guaranteed present by the time any of these
/// lazies first resolve (`lib/kobako/errors.rb` loads the hierarchy before
/// the ext raises into it), so a missing constant is a build / wiring bug
/// and the `unwrap` is the correct fail-fast.
fn kobako_error_class(ruby: &Ruby, name: &str) -> ExceptionClass {
    let kobako: RModule = ruby.class_object().const_get("Kobako").unwrap();
    kobako.const_get(name).unwrap()
}

pub(crate) static SETUP_ERROR: Lazy<ExceptionClass> =
    Lazy::new(|ruby| kobako_error_class(ruby, "SetupError"));

pub(crate) static MODULE_NOT_BUILT_ERROR: Lazy<ExceptionClass> =
    Lazy::new(|ruby| kobako_error_class(ruby, "ModuleNotBuiltError"));

pub(crate) static TRAP_ERROR: Lazy<ExceptionClass> =
    Lazy::new(|ruby| kobako_error_class(ruby, "TrapError"));

pub(crate) static TIMEOUT_ERROR: Lazy<ExceptionClass> =
    Lazy::new(|ruby| kobako_error_class(ruby, "TimeoutError"));

pub(crate) static MEMORY_LIMIT_ERROR: Lazy<ExceptionClass> =
    Lazy::new(|ruby| kobako_error_class(ruby, "MemoryLimitError"));

pub(crate) static SANDBOX_ERROR: Lazy<ExceptionClass> =
    Lazy::new(|ruby| kobako_error_class(ruby, "SandboxError"));

/// Build a `MagnusError` in `class` carrying `msg` — the shared body of
/// the named `*_err` constructors below, which differ only in which
/// error-class `Lazy` they target.
fn error_in(ruby: &Ruby, class: &Lazy<ExceptionClass>, msg: impl Into<String>) -> MagnusError {
    MagnusError::new(ruby.get_inner(class), msg.into())
}

/// Construct a `Kobako::TrapError` magnus error. Used for every
/// invocation-time wasmtime engine failure that is not a configured-cap
/// trap — missing exports, allocation faults, memory write/read failures.
/// Construction-time setup failures use `setup_err`, not this.
pub(crate) fn trap_err(ruby: &Ruby, msg: impl Into<String>) -> MagnusError {
    error_in(ruby, &TRAP_ERROR, msg)
}

/// Construct a `Kobako::SetupError` magnus error. Used for every
/// construction-time failure on the `Runtime.from_path` path before any
/// invocation runs — unreadable artifact, bytes that are not a valid Wasm
/// module, or engine / linker / instantiation setup failure (docs/behavior.md
/// E-41). The `ModuleNotBuiltError` subclass (artifact absent, E-40) is
/// raised through `MODULE_NOT_BUILT_ERROR` directly.
pub(crate) fn setup_err(ruby: &Ruby, msg: impl Into<String>) -> MagnusError {
    error_in(ruby, &SETUP_ERROR, msg)
}

/// Construct a `Kobako::TimeoutError` magnus error. Surfaces the
/// docs/behavior.md E-19 wall-clock cap path with the verb prefix added
/// by `Kobako::Sandbox#invoke!`.
pub(crate) fn timeout_err(ruby: &Ruby, msg: impl Into<String>) -> MagnusError {
    error_in(ruby, &TIMEOUT_ERROR, msg)
}

/// Construct a `Kobako::MemoryLimitError` magnus error. Surfaces the
/// docs/behavior.md E-20 linear-memory cap path with the verb prefix
/// added by `Kobako::Sandbox#invoke!`.
pub(crate) fn memory_limit_err(ruby: &Ruby, msg: impl Into<String>) -> MagnusError {
    error_in(ruby, &MEMORY_LIMIT_ERROR, msg)
}

/// Construct a `Kobako::SandboxError` magnus error. Used for the
/// host-side pre-call faults the SPEC attributes to the sandbox / wire
/// layer rather than the Wasm engine — currently the `#run` invocation
/// envelope reservation failure (`__kobako_alloc` returns 0,
/// docs/behavior.md E-31). The runtime is intact, so this must not be a
/// `TrapError`: no discard-and-recreate recovery is owed to the caller.
pub(crate) fn sandbox_err(ruby: &Ruby, msg: impl Into<String>) -> MagnusError {
    error_in(ruby, &SANDBOX_ERROR, msg)
}

// ---------------------------------------------------------------------------
// Ruby init
// ---------------------------------------------------------------------------

pub fn init(ruby: &Ruby, kobako: RModule) -> Result<(), MagnusError> {
    // Error hierarchy lives in `lib/kobako/errors.rb` (top-level
    // `Kobako::TrapError` / `TimeoutError` / `MemoryLimitError` /
    // `SetupError` / `ModuleNotBuiltError`). The ext raises directly into
    // those classes through `trap_err` / `timeout_err` / `memory_limit_err`
    // / `sandbox_err` / `setup_err` / `MODULE_NOT_BUILT_ERROR`; no
    // intermediate hierarchy is registered.

    let runtime = kobako.define_class("Runtime", ruby.class_object())?;
    runtime.define_singleton_method("from_path", function!(Runtime::from_path, 5))?;
    runtime.define_method("on_dispatch=", method!(Runtime::set_on_dispatch, 1))?;
    runtime.define_method(
        "yield_to_active_invocation",
        method!(Runtime::yield_to_active_invocation, 1),
    )?;
    runtime.define_method("eval", method!(Runtime::eval, 3))?;
    runtime.define_method("run", method!(Runtime::run, 3))?;
    runtime.define_method("usage", method!(Runtime::usage, 0))?;

    Ok(())
}

#[derive(TypedData)]
#[magnus(class = "Kobako::Runtime", free_immediately, size, mark)]
pub(crate) struct Runtime {
    inner: WtInstance,
    store: StoreCell,
    // Cached host-driven ABI export handles (`__kobako_eval` / `_run` /
    // `_take_outcome`); see `Exports`. `__kobako_alloc` is not among them
    // — only `dispatch.rs` calls it, via `Caller::get_export`.
    exports: Exports,
    // Wall-clock + per-channel capture caps forwarded from the Sandbox;
    // see `Config`. Distinct from the per-invocation `memory_limit`,
    // which lives on `Invocation` because the wasmtime `ResourceLimiter`
    // callback consumes it from inside the wasm engine.
    config: Config,
    // The host-side dispatch Proc (docs/behavior.md B-12), held here only
    // to give `DataTypeFunctions::mark` a Store-free read path so it can
    // pin the Proc across GC. The copy the `__kobako_dispatch` import
    // actually calls lives on `Invocation` (reached through
    // `Caller<Invocation>`, which cannot see this struct); see
    // `Runtime::set_on_dispatch`. Both hold the same `Copy` handle to the
    // one pinned Proc. `Cell` is sound under the GVL (see the `unsafe impl
    // Sync` below).
    on_dispatch: Cell<Option<Opaque<Value>>>,
}

impl DataTypeFunctions for Runtime {
    /// Mark — and thereby pin — the host-side dispatch Proc so Ruby's GC
    /// neither collects nor moves it while the ext holds a raw `Opaque`
    /// copy on `Invocation` for the duration of a guest invocation.
    /// `gc::Marker::mark` maps to `rb_gc_mark`, which pins: required because
    /// the Invocation copy is a cached `VALUE` that compaction would
    /// otherwise leave dangling (docs/behavior.md B-12 / B-13). Without
    /// this the Proc has no GC root at all — sweep collects it (SIGSEGV on
    /// the next dispatch) and compaction relocates it (dispatch lands on
    /// the wrong receiver).
    fn mark(&self, marker: &gc::Marker) {
        if let Some(on_dispatch) = self.on_dispatch.get() {
            marker.mark(on_dispatch);
        }
    }
}

// SAFETY: magnus requires `Send + Sync` on TypedData types. The added
// `on_dispatch: Cell<…>` makes the auto-derived `Sync` unavailable, but the
// same GVL invariant that justifies `StoreCell`'s assertion applies here:
// every access to the Cell happens under the GVL on a single thread at a
// time — `set_on_dispatch` from a Ruby method call, and `mark` from a GC
// pass that also holds the GVL. No cross-thread access to the Cell can
// occur. `Send` stays auto-derived (`Opaque<Value>` is `Send`).
unsafe impl Sync for Runtime {}

impl Runtime {
    /// Construct an Runtime from a wasm file path, using the process-wide
    /// shared Engine and per-path Module cache. The single Ruby-facing
    /// constructor for `Kobako::Runtime` — Engine and Module are never
    /// visible to Ruby.
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
                // docs/behavior.md E-39: an invalid cap argument is a Host App
                // programming error and raises `ArgumentError`, outside the
                // construction-failure `SetupError` branch. `SandboxOptions`
                // is the primary guard (it never lets a bad timeout reach
                // here); this is defence-in-depth for direct `from_path` calls.
                return Err(MagnusError::new(
                    ruby.exception_arg_error(),
                    format!("timeout must be > 0 and finite, got {secs} seconds"),
                ));
            }
        };

        let engine = shared_engine()?;
        let module = cached_module(Path::new(&path))?;

        let mut store = WtStore::new(engine, Invocation::new(memory_limit));
        store.limiter(|state: &mut Invocation| -> &mut dyn ResourceLimiter { state.limiter_mut() });
        store.epoch_deadline_callback(trap::epoch_deadline_callback);

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

    /// Build an `Runtime` from an engine, module, and store cell. The
    /// store cell is moved in and ends up owned by the returned Runtime.
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
        let mut linker: Linker<Invocation> = Linker::new(engine);

        // Wire the wasmtime-wasi preview1 WASI imports. Routes guest fd 1/2
        // to the MemoryOutputPipes set up before each run via
        // `Runtime::eval`. The closure pulls a `&mut WasiP1Ctx` out of
        // Invocation; the panic semantics live inside `Invocation::wasi_mut`
        // so the wiring stays honest about its precondition.
        p1::add_to_linker_sync(&mut linker, |state: &mut Invocation| state.wasi_mut())
            .map_err(|e| setup_err(&ruby, format!("failed to set up the WASI runtime: {}", e)))?;

        // `__kobako_dispatch` host import. Signature per docs/wire-codec.md
        // § ABI Signatures:
        //   (req_ptr: i32, req_len: i32) -> i64
        // Decodes the Request bytes, dispatches via the Ruby-side
        // dispatch Proc (bound per-Sandbox through `Runtime#on_dispatch=`),
        // allocates a guest buffer through `__kobako_alloc`, writes
        // the Response bytes there, and returns the packed
        // `(ptr<<32)|len`. The dispatcher returns 0 on any wire-layer
        // fault (including no Proc bound); see `dispatch::handle`.
        linker
            .func_wrap(
                "env",
                "__kobako_dispatch",
                |mut caller: Caller<'_, Invocation>, req_ptr: i32, req_len: i32| -> i64 {
                    dispatch::handle(&mut caller, req_ptr, req_len)
                },
            )
            .map_err(|e| {
                setup_err(
                    &ruby,
                    format!("failed to set up the host callback bridge: {}", e),
                )
            })?;

        let instance = {
            let mut store_ref = store_cell.borrow_mut();
            linker
                .instantiate(store_ref.as_context_mut(), module)
                .map_err(|e| trap::instantiate_err(&ruby, e))?
        };

        Self::validate_abi_version(&instance, &store_cell, &ruby)?;

        let exports = Exports::resolve(&instance, &store_cell);

        Ok(Self {
            inner: instance,
            store: store_cell,
            exports,
            config: Config {
                timeout,
                stdout_limit_bytes,
                stderr_limit_bytes,
            },
            on_dispatch: Cell::new(None),
        })
    }

    /// Probe the guest's `__kobako_abi_version` export once at
    /// construction and require equality with `ABI_VERSION`
    /// (docs/behavior.md B-40). An absent export or a non-equal value is
    /// E-42 — a deterministic artifact fault raised as
    /// `Kobako::SetupError`.
    fn validate_abi_version(
        instance: &WtInstance,
        store: &StoreCell,
        ruby: &Ruby,
    ) -> Result<(), MagnusError> {
        let mut store_ref = store.borrow_mut();
        let mut ctx = store_ref.as_context_mut();
        let probe = instance
            .get_typed_func::<(), u32>(&mut ctx, "__kobako_abi_version")
            .map_err(|_| {
                setup_err(
                    ruby,
                    format!(
                        "the Guest Binary does not export __kobako_abi_version; \
                         rebuild it against ABI version {ABI_VERSION}"
                    ),
                )
            })?;
        let reported = probe.call(&mut ctx, ()).map_err(|e| {
            setup_err(
                ruby,
                format!("failed to read the Guest Binary's ABI version: {e}"),
            )
        })?;
        if reported != ABI_VERSION {
            return Err(setup_err(
                ruby,
                format!(
                    "the Guest Binary reports ABI version {reported}, but this host \
                     implements ABI version {ABI_VERSION}; rebuild the Guest Binary \
                     against the host's version"
                ),
            ));
        }
        Ok(())
    }

    /// Register the Ruby-side dispatch `Proc` on the active Invocation.
    /// Bound to Ruby as `Kobako::Runtime#on_dispatch=`. From this point on,
    /// every `__kobako_dispatch` host import invocation calls the Proc
    /// with the request bytes and writes the returned Response bytes back
    /// into guest memory (docs/behavior.md B-12).
    pub(crate) fn set_on_dispatch(&self, proc_value: Value) -> Result<(), MagnusError> {
        let on_dispatch = Opaque::from(proc_value);
        // Write both copies of the one Proc handle: the `on_dispatch` Cell
        // gives `DataTypeFunctions::mark` a Store-free read path to pin the
        // Proc across GC, and the `Invocation` copy is what the
        // `__kobako_dispatch` import reads through `Caller<Invocation>`.
        // `mark` cannot reach the Invocation copy itself — the Store is held
        // `borrow_mut` for the whole guest call, exactly when GC may fire
        // during dispatch — so the Cell is the dedicated GC-rooting anchor.
        self.on_dispatch.set(Some(on_dispatch));
        self.store
            .borrow_mut()
            .data_mut()
            .bind_on_dispatch(on_dispatch);
        Ok(())
    }

    /// Synchronously re-enter the guest's `__kobako_yield_to_block`
    /// export with `args_bytes` as the yield-arguments payload, and
    /// return the YieldResponse bytes the guest produced (B-24).
    ///
    /// Bound to Ruby as `Kobako::Runtime#yield_to_active_invocation`.
    /// Recovers the dispatcher's `&mut Caller` from the per-thread
    /// Invocation slot (SPEC.md Single-Invocation Slot) — the host is
    /// already inside a `__kobako_dispatch` callback, so the Caller
    /// parked on the Rust stack is the same one the Sandbox-level
    /// `#eval` / `#run` is driving. Invoked from the host-side yield
    /// proxy that the dispatcher hands to Service methods (B-23 / B-24);
    /// raises `Kobako::TrapError` when called outside an active dispatch
    /// frame, or when any of the underlying allocation / write / call /
    /// read steps fails.
    pub(crate) fn yield_to_active_invocation(
        &self,
        args_bytes: RString,
    ) -> Result<RString, MagnusError> {
        let ruby = Ruby::get().expect("Ruby thread");
        let _ = self; // The Caller carries its own Store; `self` is only
                      // a marker that the method belongs to an Runtime.

        let bytes = rstring_to_vec(args_bytes);
        let Some(caller) = dispatch::current_caller() else {
            return Err(trap_err(
                &ruby,
                "yield_to_active_invocation called outside an active Sandbox dispatch frame",
            ));
        };

        let resp_bytes =
            guest_mem::drive_yield(caller, &bytes).map_err(|msg| trap_err(&ruby, msg))?;
        Ok(ruby.str_from_slice(&resp_bytes))
    }

    // -----------------------------------------------------------------
    // Run-path methods. Each method is best-effort — it raises a Ruby
    // `Kobako::TrapError` when the corresponding export is missing or
    // fails so the Sandbox layer can map errors to the three-class
    // taxonomy.
    // -----------------------------------------------------------------

    /// Execute one guest invocation (`__kobako_eval` — one-shot source)
    /// and return a `Snapshot` bundling every per-invocation observable.
    ///
    /// Rebuilds the WASI context with fresh stdin / stdout / stderr pipes
    /// (the three-frame stdin protocol carries `preamble`, `source`, then
    /// `snippets` — docs/wire-codec.md § Invocation channels), then
    /// invokes `__kobako_eval`. Per-invocation caps (docs/behavior.md
    /// B-01) are primed here: the wall-clock deadline is stamped into
    /// `Invocation` and the epoch deadline is set to fire at the next
    /// ticker tick; the memory-cap limiter is already wired.
    ///
    /// On a wasmtime trap the configured-cap path raises
    /// `Kobako::TimeoutError` / `Kobako::MemoryLimitError`; everything
    /// else raises `Kobako::TrapError`. On success the Snapshot carries
    /// the OUTCOME_BUFFER bytes, the per-channel stdout / stderr captures
    /// with their truncation flags, and the B-35 usage figures.
    pub(crate) fn eval(
        &self,
        preamble: RString,
        source: RString,
        snippets: RString,
    ) -> Result<Snapshot, MagnusError> {
        let ruby = Ruby::get().expect("Ruby thread");
        let eval = require_export(&ruby, self.exports.eval.as_ref())?;
        self.refresh_wasi(&[
            rstring_to_vec(preamble),
            rstring_to_vec(source),
            rstring_to_vec(snippets),
        ])?;
        self.call_with_caps(eval, ())
            .map_err(|e| trap::call_err(&ruby, e))?;
        self.build_snapshot(&ruby)
    }

    /// Execute one entrypoint dispatch (`__kobako_run`) and return a
    /// `Snapshot` bundling every per-invocation observable.
    ///
    /// Rebuilds the WASI context with the two-frame stdin protocol
    /// (preamble + snippets; no user source frame — docs/wire-codec.md
    /// § Invocation channels), copies `envelope` bytes into guest linear
    /// memory via `__kobako_alloc`, and calls `__kobako_run(env_ptr,
    /// env_len)`. Per-invocation cap semantics match `Runtime::eval`.
    /// Raises `Kobako::TrapError` ("alloc returned 0") when guest
    /// allocation fails (docs/behavior.md E-31).
    pub(crate) fn run(
        &self,
        preamble: RString,
        snippets: RString,
        envelope: RString,
    ) -> Result<Snapshot, MagnusError> {
        let ruby = Ruby::get().expect("Ruby thread");
        let run = require_export(&ruby, self.exports.run.as_ref())?;
        self.refresh_wasi(&[rstring_to_vec(preamble), rstring_to_vec(snippets)])?;
        let (env_ptr, env_len) = self.write_envelope(&ruby, envelope)?;
        self.call_with_caps(run, (env_ptr, env_len))
            .map_err(|e| trap::call_err(&ruby, e))?;
        self.build_snapshot(&ruby)
    }

    /// Collect every per-invocation observable into a fresh `Snapshot`.
    /// Called from the run-path methods after the guest export returns
    /// successfully: drains OUTCOME_BUFFER via `__kobako_take_outcome`,
    /// snapshots the per-channel stdout / stderr pipes (clipped to their
    /// caps), and reads B-35 `wall_time` / `memory_peak` from Invocation.
    fn build_snapshot(&self, ruby: &Ruby) -> Result<Snapshot, MagnusError> {
        let return_bytes = self.fetch_outcome_bytes(ruby)?;
        let (stdout_raw, stderr_raw, wall_time, memory_peak) = {
            let state = self.store.borrow();
            let data = state.data();
            (
                data.stdout_bytes(),
                data.stderr_bytes(),
                data.wall_time(),
                data.memory_peak(),
            )
        };
        let (stdout_visible, stdout_truncated) =
            capture::clip_capture(&stdout_raw, self.config.stdout_limit_bytes);
        let stdout_bytes = stdout_visible.to_vec();
        let (stderr_visible, stderr_truncated) =
            capture::clip_capture(&stderr_raw, self.config.stderr_limit_bytes);
        let stderr_bytes = stderr_visible.to_vec();
        Ok(Snapshot::new(
            return_bytes,
            stdout_bytes,
            stdout_truncated,
            stderr_bytes,
            stderr_truncated,
            wall_time,
            memory_peak,
        ))
    }

    /// Return the docs/behavior.md B-35 per-last-invocation usage as a
    /// Ruby 2-tuple `[wall_time, memory_peak]`. The element order
    /// matches the `Kobako::Usage` field order declared in
    /// `lib/kobako/usage.rb`; reorder both sides together if the field
    /// list ever grows.
    ///
    ///   * `wall_time` (Float seconds) — the wall-clock duration the
    ///     most recent invocation spent inside the guest export call.
    ///     Bracket opens in `Runtime::prime_caps` and closes in
    ///     `Runtime::disarm_caps`, so the value mirrors the
    ///     `timeout` deadline accounting and excludes everything that
    ///     runs after the guest export returns — the post-export
    ///     `OUTCOME_BUFFER` fetch and decode, plus stdout / stderr
    ///     capture readout. `0.0` before the first invocation.
    ///   * `memory_peak` (Integer bytes) — the high-water mark of the
    ///     per-invocation `memory.grow` delta past the linear-memory
    ///     size captured at invocation entry. `0` before the first
    ///     invocation.
    ///
    /// Packing both readers into one ext call mirrors the combined
    /// stdout / stderr readout in `Runtime::build_snapshot`: one
    /// `store.borrow()` per readout and a single magnus binding to
    /// extend when B-35's field list grows past two.
    pub(crate) fn usage(&self) -> Result<RArray, MagnusError> {
        let ruby = Ruby::get().expect("Ruby thread");
        let state = self.store.borrow();
        let data = state.data();
        let arr = ruby.ary_new_capa(2);
        arr.push(data.wall_time().as_secs_f64())?;
        arr.push(data.memory_peak())?;
        Ok(arr)
    }

    // -----------------------------------------------------------------
    // Private helpers.
    // -----------------------------------------------------------------

    /// Run one guest export call inside the per-invocation cap window:
    /// `Runtime::prime_caps` before, `Runtime::disarm_caps` after —
    /// the shared bracket for both run-path exports (`__kobako_eval` /
    /// `__kobako_run`). Disarm runs whether the call returns or traps, so
    /// the docs/behavior.md B-35 `wall_time` bracket and the E-20 memory
    /// cap always close — that close-on-trap guarantee is the reason this
    /// bracket lives in one place rather than inline at each call site.
    /// The wasmtime trap is returned unmapped; each caller wraps it
    /// through `trap::call_err` for its own error context.
    fn call_with_caps<Params, Results>(
        &self,
        export: &TypedFunc<Params, Results>,
        params: Params,
    ) -> Result<Results, wasmtime::Error>
    where
        Params: wasmtime::WasmParams,
        Results: wasmtime::WasmResults,
    {
        self.prime_caps();
        let result = {
            let mut store_ref = self.store.borrow_mut();
            export.call(store_ref.as_context_mut(), params)
        };
        self.disarm_caps();
        result
    }

    /// Stamp the per-invocation wall-clock deadline into `Invocation`
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
    /// past `baseline` counts against `memory_limit`.
    ///
    /// Also stamps the wall-clock entry instant for the
    /// docs/behavior.md B-35 `wall_time` measurement. The bracket
    /// closes in `Runtime::disarm_caps` so it matches the
    /// `timeout` deadline window and excludes `OUTCOME_BUFFER`
    /// decoding and stdout / stderr capture readout.
    fn prime_caps(&self) {
        let mut store_ref = self.store.borrow_mut();
        match self.config.timeout {
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
        store_ref.data_mut().start_wall_clock();
    }

    /// Drop the memory cap as soon as the guest call returns so that
    /// any post-run host bookkeeping (e.g. fetching the OUTCOME_BUFFER,
    /// which can grow guest memory transiently) is not attributed to
    /// the user script. Also closes the docs/behavior.md B-35
    /// `wall_time` bracket opened by `Runtime::prime_caps`. Paired
    /// with `Runtime::prime_caps`.
    fn disarm_caps(&self) {
        let mut store_ref = self.store.borrow_mut();
        store_ref.data_mut().stop_wall_clock();
        store_ref.data_mut().disarm_memory_cap();
    }

    /// Allocate a `len`-byte buffer in guest linear memory via
    /// `__kobako_alloc`, copy `envelope` into it, and return `(ptr, len)`
    /// as `i32` values matching the `__kobako_run(env_ptr, env_len)` ABI.
    /// Raises `Kobako::TrapError` when the allocation hook is missing or
    /// itself traps, and `Kobako::SandboxError` when the hook runs but
    /// cannot reserve the buffer (`__kobako_alloc` returns 0,
    /// docs/behavior.md E-31) — an intact runtime, not an engine fault.
    fn write_envelope(&self, ruby: &Ruby, envelope: RString) -> Result<(i32, i32), MagnusError> {
        let bytes = rstring_to_vec(envelope);
        let len_i32 =
            guest_mem::checked_payload_len(bytes.len()).map_err(|msg| trap_err(ruby, msg))?;

        let mut store_ref = self.store.borrow_mut();
        let alloc: TypedFunc<u32, u32> = self
            .inner
            .get_typed_func(store_ref.as_context_mut(), "__kobako_alloc")
            .map_err(|_| trap_err(ruby, SANDBOX_RUNTIME_MISSING_HOOKS))?;
        let ptr = alloc
            .call(store_ref.as_context_mut(), bytes.len() as u32)
            .map_err(|e| trap_err(ruby, format!("failed to allocate input buffer: {}", e)))?;
        if ptr == 0 {
            return Err(sandbox_err(
                ruby,
                "could not allocate input buffer (out of memory)",
            ));
        }

        let memory: Memory = match self.inner.get_export(store_ref.as_context_mut(), "memory") {
            Some(Extern::Memory(m)) => m,
            _ => return Err(trap_err(ruby, SANDBOX_RUNTIME_NOT_KOBAKO)),
        };
        let data = memory.data_mut(store_ref.as_context_mut());
        let range = guest_mem::guest_buffer_range(ptr as usize, bytes.len(), data.len())
            .map_err(|msg| trap_err(ruby, msg))?;
        data[range].copy_from_slice(&bytes);

        Ok((ptr as i32, len_i32))
    }

    /// Rebuild the WASI context with fresh stdin (carrying every frame in
    /// `frames`, each prefixed by its 4-byte big-endian u32 length —
    /// docs/wire-codec.md § Invocation channels) plus fresh stdout / stderr
    /// pipes. Called at the top of every guest invocation: `#eval` passes
    /// three frames (preamble, source, snippets), `#run` passes two
    /// (preamble, snippets — the invocation envelope arrives via linear
    /// memory instead). Each output pipe is sized at `cap + 1` so
    /// `capture::clip_capture` can distinguish "wrote exactly cap
    /// bytes" from "exceeded cap"; uncapped channels fall back
    /// to `usize::MAX` and rely on `memory_limit` (docs/behavior.md E-20)
    /// for the real ceiling. Raises `Kobako::TrapError` when any frame
    /// exceeds the 16 MiB cap that keeps its `u32` length prefix from
    /// wrapping.
    fn refresh_wasi(&self, frames: &[Vec<u8>]) -> Result<(), MagnusError> {
        let ruby = Ruby::get().expect("Ruby thread");
        // Every frame carries the same 16 MiB cap as the `#run` envelope
        // (`write_envelope`): the length prefix is a `u32`, so a frame past
        // the cap would silently wrap and corrupt the stdin frame stream.
        for frame in frames {
            guest_mem::checked_payload_len(frame.len()).map_err(|msg| trap_err(&ruby, msg))?;
        }

        let total: usize = frames.iter().map(|f| 4 + f.len()).sum();
        let mut stdin_content: Vec<u8> = Vec::with_capacity(total);
        for frame in frames {
            stdin_content.extend_from_slice(&(frame.len() as u32).to_be_bytes());
            stdin_content.extend_from_slice(frame);
        }

        let stdin_pipe = MemoryInputPipe::new(stdin_content);
        let stdout_pipe =
            MemoryOutputPipe::new(capture::pipe_capacity(self.config.stdout_limit_bytes));
        let stderr_pipe =
            MemoryOutputPipe::new(capture::pipe_capacity(self.config.stderr_limit_bytes));

        let mut builder = WasiCtxBuilder::new();
        builder.stdin(stdin_pipe);
        builder.stdout(stdout_pipe.clone());
        builder.stderr(stderr_pipe.clone());
        // Deny the preview1 ambient-authority imports the guest never legitimately
        // reaches but the WASI layer would otherwise grant (see `ambient`).
        builder.wall_clock(ambient::FrozenWallClock);
        builder.monotonic_clock(ambient::FrozenMonotonicClock);
        builder.secure_random(ambient::deterministic_rng());
        let wasi = builder.build_p1();

        self.store
            .borrow_mut()
            .data_mut()
            .install_wasi(wasi, stdout_pipe, stderr_pipe);
        Ok(())
    }

    /// Invoke `__kobako_take_outcome`, decode the packed `(ptr<<32)|len`
    /// u64, and copy the OUTCOME_BUFFER slice out of guest memory. Raises
    /// `Kobako::TrapError` when the export is missing, `len` exceeds the
    /// 16 MiB single-dispatch cap, the `ptr`/`len` arithmetic overflows,
    /// the slice falls outside live memory, or the `memory` export itself
    /// is absent.
    fn fetch_outcome_bytes(&self, ruby: &Ruby) -> Result<Vec<u8>, MagnusError> {
        let take = require_export(ruby, self.exports.take_outcome.as_ref())?;

        let mut store_ref = self.store.borrow_mut();
        let packed = take
            .call(store_ref.as_context_mut(), ())
            .map_err(|e| trap_err(ruby, format!("failed to read the Sandbox result: {}", e)))?;
        let (ptr, len) = guest_mem::unpack_outcome_packed(packed);
        if len > guest_mem::MAX_DISPATCH_PAYLOAD {
            return Err(trap_err(ruby, "result payload exceeds the 16 MiB limit"));
        }

        let mem: Memory = match self.inner.get_export(store_ref.as_context_mut(), "memory") {
            Some(Extern::Memory(m)) => m,
            _ => return Err(trap_err(ruby, SANDBOX_RUNTIME_NOT_KOBAKO)),
        };
        let data = mem.data(store_ref.as_context_mut());
        let range = guest_mem::guest_buffer_range(ptr, len, data.len()).map_err(|msg| {
            trap_err(
                ruby,
                format!("the Sandbox result is out of bounds: {}", msg),
            )
        })?;
        Ok(data[range].to_vec())
    }
}

/// User-facing message for the "Sandbox runtime is missing one of the
/// internal Kobako hooks" failure mode. Phrased in caller vocabulary —
/// the underlying ABI symbol names (`__kobako_alloc`, `__kobako_eval`,
/// `__kobako_take_outcome`) are not actionable to callers, and the
/// gem itself raises this error so a self-reference like "matches the
/// kobako gem version" reads as third-person. The actionable
/// diagnosis is "your data/kobako.wasm is out of sync; rebuild it".
const SANDBOX_RUNTIME_MISSING_HOOKS: &str = "Sandbox runtime is missing required hooks; \
     rebuild data/kobako.wasm against the installed version";

/// User-facing message for the "the loaded Wasm module is not a
/// Kobako-shaped runtime at all" failure mode (no linear memory
/// export). Same phrasing philosophy as
/// `SANDBOX_RUNTIME_MISSING_HOOKS`.
const SANDBOX_RUNTIME_NOT_KOBAKO: &str =
    "the loaded Wasm module is not a Kobako-compatible runtime";

/// Return the cached `TypedFunc` for an ABI export, or raise
/// `Kobako::TrapError` when the option is `None`. Both run-path
/// methods (`#eval`, `#run`) plus the `build_snapshot` readout that
/// drains `OUTCOME_BUFFER` share the same "missing export → Ruby
/// error" boilerplate; this helper collapses those sites onto one
/// safe entry. The user-facing message is intentionally export-
/// agnostic (see `SANDBOX_RUNTIME_MISSING_HOOKS`) — the ABI symbol
/// name is not actionable to callers, so it is not threaded in.
fn require_export<'a, Params, Results>(
    ruby: &Ruby,
    export: Option<&'a TypedFunc<Params, Results>>,
) -> Result<&'a TypedFunc<Params, Results>, MagnusError>
where
    Params: wasmtime::WasmParams,
    Results: wasmtime::WasmResults,
{
    export.ok_or_else(|| trap_err(ruby, SANDBOX_RUNTIME_MISSING_HOOKS))
}
