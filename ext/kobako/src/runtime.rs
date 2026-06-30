// Host-side wasmtime runtime wrapper.
//
// The only Ruby-visible class is
//
//   Kobako::Runtime — wraps a pre-linked InstancePre + per-Runtime caps
//
// constructed via `Kobako::Runtime.from_path(path, timeout, memory_limit,
// stdout_limit, stderr_limit)`. Every invocation (`#eval` / `#run`)
// instantiates a fresh instance from the InstancePre and discards the
// whole Store afterwards — the per-invocation instance discipline
// (ABI v2). The underlying wasmtime Engine and
// compiled Module live in a process-scope cache (see the `cache`
// submodule) and never surface to Ruby (SPEC.md "Code Organization":
// `ext/` "exposes no Wasm engine types to the Host App or downstream
// gems").
//
// Module layout (per CLAUDE.md principle #2 — one responsibility per file):
//
//   * `cache`       — process-wide Engine + per-path Module cache and the
//                     process-singleton epoch ticker thread.
//   * `config`      — per-Runtime caps (timeout / stdout / stderr limits).
//   * `exports`     — per-invocation `__kobako_eval` / `_run` /
//                     `_take_outcome` / `_alloc` / `memory` handles.
//   * `instance_pre`— host-import Linker wiring + per-path `InstancePre`
//                     cache.
//   * `invocation`  — Invocation (per-Store context), the `MemoryLimiter`
//                     memory cap, and the trap marker types
//                     (`TimeoutTrap` / `MemoryLimitTrap`).
//   * `dispatch`    — `__kobako_dispatch` host-import dispatch helpers.
//   * `guest_mem`   — Caller-based guest linear-memory alloc / write / read.
//   * `capture`     — stdout / stderr pipe sizing + clip helpers.
//   * `trap`        — wasmtime-error → `Kobako::*` trap classification.
//
// This file owns the `Kobako::Runtime` magnus class itself (the
// InstancePre + `Config` + the per-invocation `#eval` / `#run` run
// path), the Ruby error-class lazy-resolvers, the `trap_err` /
// `timeout_err` / `memory_limit_err` / `setup_err` constructors shared by
// every submodule, and the Ruby init() that registers the class.

mod ambient;
mod cache;
mod capture;
mod config;
mod dispatch;
mod errors;
mod exports;
mod frames;
mod guest_mem;
mod instance_pre;
mod invocation;
mod trap;

use magnus::{function, method, prelude::*, Error as MagnusError, RModule, RString, Ruby};

use std::cell::Cell;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use magnus::{gc, typed_data::DataTypeFunctions, value::Opaque, RArray, TypedData, Value};

use crate::contract::error::{Error, SetupError, Trap};
use crate::snapshot::Snapshot;
use wasmtime::{
    AsContextMut, InstancePre as WtInstancePre, ResourceLimiter, Store as WtStore, TypedFunc,
};

use self::cache::shared_engine;
use self::config::Config;
use self::exports::Exports;
use self::invocation::Invocation;

/// The wire ABI version this host implements (docs/wire-codec.md § ABI
/// Version). A Guest Binary is accepted only when its
/// `__kobako_abi_version` export reports the same value; a mismatch
/// is a deterministic artifact fault. The guest-side mirror is
/// `kobako_core::abi::ABI_VERSION`. Version 2
/// carries the per-invocation instance discipline: the host
/// drives every invocation on a fresh instance, so the guest may leave
/// its VM state dirty at exit.
const ABI_VERSION: u32 = 2;

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
    // Pre-linked instantiation template (import wiring + type checks
    // done once in `instance_pre::cached_instance_pre`). Every
    // invocation instantiates a fresh instance from it and discards the
    // whole Store afterwards — the per-invocation instance discipline.
    instance_pre: WtInstancePre<Invocation>,
    // Per-invocation linear-memory cap,
    // threaded into each fresh `Invocation`; lives apart from `Config`
    // because the wasmtime `ResourceLimiter` callback consumes it from
    // inside the wasm engine.
    memory_limit: Option<usize>,
    // Wall-clock + per-channel capture caps forwarded from the Sandbox;
    // see `Config`.
    config: Config,
    // The host-side dispatch Proc, held here only
    // to give `DataTypeFunctions::mark` a read path so it can pin the
    // Proc across GC. The copy the `__kobako_dispatch` import actually
    // calls is bound onto each per-invocation `Invocation` by
    // `Runtime::new_store`. Both hold the same `Copy` handle to the one
    // pinned Proc. `Cell` is sound under the GVL (see the `unsafe impl
    // Sync` below).
    on_dispatch: Cell<Option<Opaque<Value>>>,
    // Usage of the most recent invocation —
    // `(wall_time_seconds, memory_peak_bytes)` — captured by
    // `build_snapshot` before the per-invocation Store is discarded so
    // `#usage` reads survive the teardown. `(0.0, 0)` before the first
    // invocation.
    last_usage: Cell<(f64, usize)>,
}

impl DataTypeFunctions for Runtime {
    /// Mark — and thereby pin — the host-side dispatch Proc so Ruby's GC
    /// neither collects nor moves it while the ext holds a raw `Opaque`
    /// copy on `Invocation` for the duration of a guest invocation.
    /// `gc::Marker::mark` maps to `rb_gc_mark`, which pins: required because
    /// the Invocation copy is a cached `VALUE` that compaction would
    /// otherwise leave dangling. Without
    /// this the Proc has no GC root at all — sweep collects it (SIGSEGV on
    /// the next dispatch) and compaction relocates it (dispatch lands on
    /// the wrong receiver).
    fn mark(&self, marker: &gc::Marker) {
        if let Some(on_dispatch) = self.on_dispatch.get() {
            marker.mark(on_dispatch);
        }
    }
}

// SAFETY: magnus requires `Send + Sync` on TypedData types. The
// `on_dispatch` / `last_usage` `Cell`s make the auto-derived `Sync`
// unavailable, but every access to them happens under the GVL on a single
// thread at a time — Ruby method calls, and a GC `mark` pass that also
// holds the GVL. No cross-thread access to either Cell can occur. `Send`
// stays auto-derived.
unsafe impl Sync for Runtime {}

impl Runtime {
    /// Construct a Runtime from a wasm file path, using the process-wide
    /// shared Engine and per-path Module / InstancePre caches. The single
    /// Ruby-facing constructor for `Kobako::Runtime` — Engine and Module
    /// are never visible to Ruby.
    ///
    /// `timeout_seconds` is the wall-clock cap in seconds
    /// (`None` disables); `memory_limit` is the linear-memory cap in
    /// bytes (`None` disables); `stdout_limit_bytes` / `stderr_limit_bytes`
    /// are the per-channel output caps (`None`
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
                // An invalid cap argument is a Host App
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

        let instance_pre = instance_pre::cached_instance_pre(Path::new(&path))
            .map_err(|e| errors::setup_to_magnus(&ruby, e))?;
        let runtime = Self {
            instance_pre,
            memory_limit,
            config: Config {
                timeout,
                stdout_limit_bytes,
                stderr_limit_bytes,
            },
            on_dispatch: Cell::new(None),
            last_usage: Cell::new((0.0, 0)),
        };
        runtime.probe_abi_version(&ruby)?;
        Ok(runtime)
    }

    /// Instantiate a throwaway probe instance at construction and require
    /// the guest's `__kobako_abi_version` export to equal `ABI_VERSION`
    /// An absent export or a non-equal value is
    /// a deterministic artifact fault raised as
    /// `Kobako::SetupError`. The probe Store drops here; invocation
    /// instances are created per `#eval` / `#run`. The frameless WASI
    /// context keeps a third-party guest whose start section touches
    /// WASI on the `SetupError` path instead of panicking in
    /// `Invocation::wasi_mut`.
    fn probe_abi_version(&self, ruby: &Ruby) -> Result<(), MagnusError> {
        let mut store = self
            .new_store()
            .map_err(|e| errors::setup_to_magnus(ruby, e))?;
        frames::install_wasi_frames(&mut store, &self.config, &[])
            .map_err(|t| errors::setup_err(ruby, t.to_string()))?;
        let instance = self
            .instance_pre
            .instantiate(store.as_context_mut())
            .map_err(|e| errors::setup_to_magnus(ruby, trap::instantiate_err(e)))?;
        let probe = instance
            .get_typed_func::<(), u32>(store.as_context_mut(), "__kobako_abi_version")
            .map_err(|_| {
                errors::setup_err(
                    ruby,
                    format!(
                        "the Guest Binary does not export __kobako_abi_version; \
                         rebuild it against ABI version {ABI_VERSION}"
                    ),
                )
            })?;
        let reported = probe.call(store.as_context_mut(), ()).map_err(|e| {
            errors::setup_err(
                ruby,
                format!("failed to read the Guest Binary's ABI version: {e}"),
            )
        })?;
        if reported != ABI_VERSION {
            return Err(errors::setup_err(
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

    /// Register the Ruby-side dispatch `Proc`.
    /// Bound to Ruby as `Kobako::Runtime#on_dispatch=`. The handle is
    /// pinned by `DataTypeFunctions::mark` and copied onto every
    /// per-invocation `Invocation` by `Runtime::new_store`, where the
    /// `__kobako_dispatch` import reads it through `Caller<Invocation>`.
    pub(crate) fn set_on_dispatch(&self, proc_value: Value) -> Result<(), MagnusError> {
        self.on_dispatch.set(Some(Opaque::from(proc_value)));
        Ok(())
    }

    /// Synchronously re-enter the guest's `__kobako_yield_to_block`
    /// export with `args_bytes` as the yield-arguments payload, and
    /// return the YieldResponse bytes the guest produced.
    ///
    /// Bound to Ruby as `Kobako::Runtime#yield_to_active_invocation`.
    /// Recovers the dispatcher's `&mut Caller` from the per-thread
    /// Invocation slot (SPEC.md Single-Invocation Slot) — the host is
    /// already inside a `__kobako_dispatch` callback, so the Caller
    /// parked on the Rust stack is the same one the Sandbox-level
    /// `#eval` / `#run` is driving. Invoked from the host-side yield
    /// proxy that the dispatcher hands to Service methods;
    /// raises `Kobako::TrapError` when called outside an active dispatch
    /// frame, or when any of the underlying allocation / write / call /
    /// read steps fails.
    pub(crate) fn yield_to_active_invocation(
        &self,
        args_bytes: RString,
    ) -> Result<RString, MagnusError> {
        let ruby = Ruby::get().expect("Ruby thread");
        let _ = self; // The Caller carries its own Store; `self` is only
                      // a marker that the method belongs to a Runtime.

        let bytes = rstring_to_vec(args_bytes);
        let Some(caller) = dispatch::current_caller() else {
            return Err(errors::trap_err(
                &ruby,
                "yield_to_active_invocation called outside an active Sandbox dispatch frame",
            ));
        };

        let resp_bytes =
            guest_mem::drive_yield(caller, &bytes).map_err(|msg| errors::trap_err(&ruby, msg))?;
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
    /// Builds a fresh Store + instance whose WASI context carries
    /// the three-frame stdin protocol (`preamble`, `source`, `snippets`
    /// — docs/wire-codec.md § Invocation channels), then invokes
    /// `__kobako_eval`. Per-invocation caps are
    /// primed here: the wall-clock deadline is stamped into `Invocation`
    /// and the epoch deadline is set to fire at the next ticker tick;
    /// the memory-cap limiter is already wired.
    ///
    /// On a wasmtime trap the configured-cap path raises
    /// `Kobako::TimeoutError` / `Kobako::MemoryLimitError`; everything
    /// else raises `Kobako::TrapError`. On success the Snapshot carries
    /// the OUTCOME_BUFFER bytes, the per-channel stdout / stderr captures
    /// with their truncation flags, and the usage figures.
    pub(crate) fn eval(
        &self,
        preamble: RString,
        source: RString,
        snippets: RString,
    ) -> Result<Snapshot, MagnusError> {
        let ruby = Ruby::get().expect("Ruby thread");
        self.eval_inner(preamble, source, snippets)
            .map_err(|e| errors::to_magnus(&ruby, e))
    }

    /// Magnus-free body of `#eval`: drive the guest export and return a
    /// `Snapshot` or a neutral run-path `Error`. The magnus method above is
    /// the single point that maps the `Error` onto a `Kobako::*` exception.
    fn eval_inner(
        &self,
        preamble: RString,
        source: RString,
        snippets: RString,
    ) -> Result<Snapshot, Error> {
        let mut store = self.new_store()?;
        frames::install_wasi_frames(
            &mut store,
            &self.config,
            &[
                rstring_to_vec(preamble),
                rstring_to_vec(source),
                rstring_to_vec(snippets),
            ],
        )?;
        let exports = self.instantiate(&mut store)?;
        let eval = frames::require_export(exports.eval.as_ref())?;
        self.call_with_caps(&mut store, &exports, eval, ())
            .map_err(trap::trap_from)?;
        Ok(self.build_snapshot(&mut store, &exports)?)
    }

    /// Execute one entrypoint dispatch (`__kobako_run`) and return a
    /// `Snapshot` bundling every per-invocation observable.
    ///
    /// Builds a fresh Store + instance whose WASI context carries
    /// the two-frame stdin protocol (preamble + snippets; no user source
    /// frame — docs/wire-codec.md § Invocation channels), copies
    /// `envelope` bytes into guest linear memory via `__kobako_alloc`,
    /// and calls `__kobako_run(env_ptr, env_len)`. Per-invocation cap
    /// semantics match `Runtime::eval`. Raises `Kobako::TrapError`
    /// ("alloc returned 0") when guest allocation fails.
    pub(crate) fn run(
        &self,
        preamble: RString,
        snippets: RString,
        envelope: RString,
    ) -> Result<Snapshot, MagnusError> {
        let ruby = Ruby::get().expect("Ruby thread");
        self.run_inner(preamble, snippets, envelope)
            .map_err(|e| errors::to_magnus(&ruby, e))
    }

    /// Magnus-free body of `#run`: copy the envelope into guest memory,
    /// drive the guest export, and return a `Snapshot` or a neutral
    /// run-path `Error`. The magnus method above maps the `Error`.
    fn run_inner(
        &self,
        preamble: RString,
        snippets: RString,
        envelope: RString,
    ) -> Result<Snapshot, Error> {
        let mut store = self.new_store()?;
        frames::install_wasi_frames(
            &mut store,
            &self.config,
            &[rstring_to_vec(preamble), rstring_to_vec(snippets)],
        )?;
        let exports = self.instantiate(&mut store)?;
        let run = frames::require_export(exports.run.as_ref())?;
        let (env_ptr, env_len) = frames::write_envelope(&mut store, &exports, envelope)?;
        self.call_with_caps(&mut store, &exports, run, (env_ptr, env_len))
            .map_err(trap::trap_from)?;
        Ok(self.build_snapshot(&mut store, &exports)?)
    }

    /// Return the per-last-invocation usage as a
    /// Ruby 2-tuple `[wall_time, memory_peak]`. The element order
    /// matches the `Kobako::Usage` field order declared in
    /// `lib/kobako/usage.rb`; reorder both sides together if the field
    /// list ever grows.
    ///
    ///   * `wall_time` (Float seconds) — the wall-clock duration the
    ///     most recent invocation spent inside the guest export call.
    ///     Bracket opens in `Runtime::prime_caps` and closes in
    ///     `disarm_caps`, so the value mirrors the `timeout` deadline
    ///     accounting and excludes everything that runs after the guest
    ///     export returns. `0.0` before the first invocation.
    ///   * `memory_peak` (Integer bytes) — the high-water mark of the
    ///     per-invocation `memory.grow` delta past the linear-memory
    ///     size captured at invocation entry. `0` before the first
    ///     invocation.
    ///
    /// Reads the `last_usage` Cell `build_snapshot` populated before the
    /// per-invocation Store was discarded.
    pub(crate) fn usage(&self) -> Result<RArray, MagnusError> {
        let ruby = Ruby::get().expect("Ruby thread");
        let (wall_time, memory_peak) = self.last_usage.get();
        let arr = ruby.ary_new_capa(2);
        arr.push(wall_time)?;
        arr.push(memory_peak)?;
        Ok(arr)
    }

    // -----------------------------------------------------------------
    // Private helpers.
    // -----------------------------------------------------------------

    /// Build the per-invocation Store: a fresh `Invocation` wired with
    /// the memory limiter, the epoch-deadline callback, and the
    /// registered dispatch Proc.
    fn new_store(&self) -> Result<WtStore<Invocation>, SetupError> {
        let mut store = WtStore::new(shared_engine()?, Invocation::new(self.memory_limit));
        store.limiter(|state: &mut Invocation| -> &mut dyn ResourceLimiter { state.limiter_mut() });
        store.epoch_deadline_callback(trap::epoch_deadline_callback);
        if let Some(on_dispatch) = self.on_dispatch.get() {
            store
                .data_mut()
                .bind_on_dispatch(Arc::new(dispatch::RubyDispatchHandler::new(on_dispatch)));
        }
        Ok(store)
    }

    /// Instantiate the per-invocation instance from the pre-linked
    /// template and resolve its host-driven export handles. An
    /// instantiation failure at invocation time is an engine fault —
    /// `Kobako::TrapError` — unlike the construction-time probe, whose
    /// failure is `SetupError`.
    fn instantiate(&self, store: &mut WtStore<Invocation>) -> Result<Exports, Trap> {
        let instance = self
            .instance_pre
            .instantiate(store.as_context_mut())
            .map_err(|e| Trap::Other(format!("failed to instantiate the Sandbox runtime: {e}")))?;
        Ok(Exports::resolve(&instance, store.as_context_mut()))
    }

    /// Run one guest export call inside the per-invocation cap window:
    /// `Runtime::prime_caps` before, `disarm_caps` after — the shared
    /// bracket for both run-path exports (`__kobako_eval` /
    /// `__kobako_run`). Disarm runs whether the call returns or traps, so
    /// the `wall_time` bracket and the memory
    /// cap always close — that close-on-trap guarantee is the reason this
    /// bracket lives in one place rather than inline at each call site.
    /// The wasmtime trap is returned unmapped; each caller wraps it
    /// through `trap::call_err` for its own error context.
    fn call_with_caps<Params, Results>(
        &self,
        store: &mut WtStore<Invocation>,
        exports: &Exports,
        export: &TypedFunc<Params, Results>,
        params: Params,
    ) -> Result<Results, wasmtime::Error>
    where
        Params: wasmtime::WasmParams,
        Results: wasmtime::WasmResults,
    {
        self.prime_caps(store, exports);
        let result = export.call(store.as_context_mut(), params);
        disarm_caps(store);
        // Stash the usage figures on every outcome — including the
        // trap paths, where `build_snapshot` never runs and the Store is
        // about to be discarded with the error.
        let data = store.data();
        self.last_usage
            .set((data.wall_time().as_secs_f64(), data.memory_peak()));
        result
    }

    /// Stamp the per-invocation wall-clock deadline into `Invocation`
    /// and prime the wasmtime epoch deadline so the next ticker tick
    /// wakes the epoch-deadline callback. When `timeout` is disabled,
    /// the deadline is set far enough in the future that the callback
    /// effectively never fires.
    ///
    /// Also captures the current linear-memory size as the baseline
    /// for the per-invocation memory delta cap —
    /// the pre-initialized image's allocation is folded into the
    /// baseline rather than the budget — and stamps the wall-clock
    /// entry instant for the `wall_time`
    /// measurement. The bracket closes in `disarm_caps` so it matches
    /// the `timeout` deadline window and excludes `OUTCOME_BUFFER`
    /// decoding and stdout / stderr capture readout.
    fn prime_caps(&self, store: &mut WtStore<Invocation>, exports: &Exports) {
        match self.config.timeout {
            Some(timeout) => {
                let deadline = Instant::now() + timeout;
                store.data_mut().set_deadline(Some(deadline));
                store.set_epoch_deadline(1);
            }
            None => {
                store.data_mut().set_deadline(None);
                store.set_epoch_deadline(u64::MAX);
            }
        }
        let baseline = match exports.memory {
            Some(m) => m.data_size(store.as_context_mut()),
            None => 0,
        };
        store.data_mut().arm_memory_cap(baseline);
        store.data_mut().start_wall_clock();
    }

    /// Collect every per-invocation observable into a fresh `Snapshot`.
    /// Called from the run-path methods after the guest export returns
    /// successfully: drains OUTCOME_BUFFER via `__kobako_take_outcome`
    /// and snapshots the per-channel stdout / stderr pipes (clipped to
    /// their caps). The usage figures were already stashed by
    /// `call_with_caps`.
    fn build_snapshot(
        &self,
        store: &mut WtStore<Invocation>,
        exports: &Exports,
    ) -> Result<Snapshot, Trap> {
        let return_bytes = frames::fetch_outcome_bytes(store, exports)?;
        let data = store.data();
        let (stdout_raw, stderr_raw, wall_time, memory_peak) = (
            data.stdout_bytes(),
            data.stderr_bytes(),
            data.wall_time(),
            data.memory_peak(),
        );
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
}

/// Drop the memory cap as soon as the guest call returns so that
/// any post-run host bookkeeping (e.g. fetching the OUTCOME_BUFFER,
/// which can grow guest memory transiently) is not attributed to
/// the user script. Also closes the
/// `wall_time` bracket opened by `Runtime::prime_caps`. Paired
/// with `Runtime::prime_caps`.
fn disarm_caps(store: &mut WtStore<Invocation>) {
    store.data_mut().stop_wall_clock();
    store.data_mut().disarm_memory_cap();
}
