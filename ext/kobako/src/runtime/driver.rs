//! The magnus-free wasmtime driver: everything needed to run one guest
//! invocation, expressed purely in contract and wasmtime types.
//!
//! A `Driver` is the engine half of `Kobako::Runtime` — the pre-linked
//! `InstancePre` plus the per-Runtime caps — and implements the contract
//! `Runtime` trait over it. Keeping it free of any frontend type lets it
//! move to a standalone engine crate unchanged; the magnus shell in
//! `crate::runtime` only shuttles Ruby values across its boundary.

use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use wasmtime::{
    AsContextMut, InstancePre as WtInstancePre, ResourceLimiter, Store as WtStore, TypedFunc,
};

use super::cache::shared_engine;
use super::config::Config;
use super::exports::Exports;
use super::invocation::Invocation;
use super::{capture, frames, instance_pre, trap};
use crate::contract::dispatch::DispatchHandler;
use crate::contract::error::{Error, SetupError, Trap};
use crate::contract::runtime::{Entry, Frames, Runtime as ContractRuntime};
use crate::contract::snapshot::{Capture, Completion, Snapshot, Usage};

/// The wire ABI version this host implements (docs/wire-codec.md § ABI
/// Version). A Guest Binary is accepted only when its
/// `__kobako_abi_version` export reports the same value; a mismatch
/// is a deterministic artifact fault. The guest-side mirror is
/// `kobako_core::abi::ABI_VERSION`. Version 2
/// carries the per-invocation instance discipline: the host
/// drives every invocation on a fresh instance, so the guest may leave
/// its VM state dirty at exit.
const ABI_VERSION: u32 = 2;

/// The wasmtime execution unit behind one `Kobako::Runtime`.
pub(super) struct Driver {
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
}

impl Driver {
    /// Construct a Driver from a wasm file path, using the process-wide
    /// shared Engine and per-path Module / InstancePre caches, and verify
    /// the artifact's ABI version. Every failure is a `SetupError` for
    /// the frontend to attribute — Engine and Module never leave the
    /// driver.
    pub(super) fn new(
        path: &Path,
        memory_limit: Option<usize>,
        config: Config,
    ) -> Result<Self, SetupError> {
        let instance_pre = instance_pre::cached_instance_pre(path)?;
        let driver = Self {
            instance_pre,
            memory_limit,
            config,
        };
        driver.probe_abi_version()?;
        Ok(driver)
    }

    /// Instantiate a throwaway probe instance at construction and require
    /// the guest's `__kobako_abi_version` export to equal `ABI_VERSION`.
    /// An absent export or a non-equal value is a deterministic artifact
    /// fault. The probe Store drops here; invocation instances are
    /// created per invoke. The frameless WASI context keeps a third-party
    /// guest whose start section touches WASI on the `SetupError` path
    /// instead of panicking in `Invocation::wasi_mut`.
    fn probe_abi_version(&self) -> Result<(), SetupError> {
        let mut store = self.new_store()?;
        frames::install_wasi_frames(&mut store, &self.config, &[])
            .map_err(|t| SetupError::Dead(t.to_string()))?;
        let instance = self
            .instance_pre
            .instantiate(store.as_context_mut())
            .map_err(trap::instantiate_err)?;
        let probe = instance
            .get_typed_func::<(), u32>(store.as_context_mut(), "__kobako_abi_version")
            .map_err(|_| {
                SetupError::Dead(format!(
                    "the Guest Binary does not export __kobako_abi_version; \
                     rebuild it against ABI version {ABI_VERSION}"
                ))
            })?;
        let reported = probe.call(store.as_context_mut(), ()).map_err(|e| {
            SetupError::Dead(format!(
                "failed to read the Guest Binary's ABI version: {e}"
            ))
        })?;
        if reported != ABI_VERSION {
            return Err(SetupError::Dead(format!(
                "the Guest Binary reports ABI version {reported}, but this host \
                 implements ABI version {ABI_VERSION}; rebuild the Guest Binary \
                 against the host's version"
            )));
        }
        Ok(())
    }

    /// Build the per-invocation Store: a fresh `Invocation` wired with
    /// the memory limiter and the epoch-deadline callback.
    fn new_store(&self) -> Result<WtStore<Invocation>, SetupError> {
        let mut store = WtStore::new(shared_engine()?, Invocation::new(self.memory_limit));
        store.limiter(|state: &mut Invocation| -> &mut dyn ResourceLimiter { state.limiter_mut() });
        store.epoch_deadline_callback(trap::epoch_deadline_callback);
        Ok(store)
    }

    /// Instantiate the per-invocation instance from the pre-linked
    /// template and resolve its host-driven export handles. An
    /// instantiation failure at invocation time is an engine fault —
    /// a `Trap` — unlike the construction-time probe, whose failure is
    /// `SetupError`.
    fn instantiate(&self, store: &mut WtStore<Invocation>) -> Result<Exports, Trap> {
        let instance = self
            .instance_pre
            .instantiate(store.as_context_mut())
            .map_err(|e| Trap::Other(format!("failed to instantiate the Sandbox runtime: {e}")))?;
        Ok(Exports::resolve(&instance, store.as_context_mut()))
    }

    /// Run one guest export call inside the per-invocation cap window:
    /// `Driver::prime_caps` before, `disarm_caps` after — the shared
    /// bracket for both run-path exports (`__kobako_eval` /
    /// `__kobako_run`). Disarm runs whether the call returns or traps, so
    /// the `wall_time` bracket and the memory
    /// cap always close — that close-on-trap guarantee is the reason this
    /// bracket lives in one place rather than inline at each call site.
    /// The wasmtime trap is returned unmapped; the caller classifies it
    /// through `trap::trap_from`.
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

    /// Bundle one invocation's observables into a fresh `Snapshot`,
    /// uniformly for every `completion` — the clipped captures and the
    /// cap-bracket usage must survive a trap just as they do an outcome.
    fn build_snapshot(&self, store: &WtStore<Invocation>, completion: Completion) -> Snapshot {
        let data = store.data();
        let usage = Usage {
            wall_time: data.wall_time().as_secs_f64(),
            memory_peak: data.memory_peak(),
        };
        let (stdout_raw, stderr_raw) = (data.stdout_bytes(), data.stderr_bytes());
        let (stdout_visible, stdout_truncated) =
            capture::clip_capture(&stdout_raw, self.config.stdout_limit_bytes);
        let (stderr_visible, stderr_truncated) =
            capture::clip_capture(&stderr_raw, self.config.stderr_limit_bytes);
        Snapshot {
            completion,
            stdout: Capture {
                bytes: stdout_visible.to_vec(),
                truncated: stdout_truncated,
            },
            stderr: Capture {
                bytes: stderr_visible.to_vec(),
                truncated: stderr_truncated,
            },
            usage,
        }
    }
}

impl ContractRuntime for Driver {
    /// Drive one guest invocation on a fresh instance and return its
    /// `Snapshot`, `Ok` iff the guest export ran. Builds a fresh Store,
    /// binds the borrowed dispatch handler, installs the stdin frames
    /// (three for `Eval` — preamble / source / snippets; two for `Run` —
    /// preamble / snippets, with the envelope copied into guest memory),
    /// and primes the per-invocation caps around the export call. A fault
    /// before the export call is the `Err` channel; once the call starts,
    /// every fault folds into the Snapshot's `Completion` — the
    /// configured-cap paths as `Trap::Timeout` / `Trap::MemoryLimit`,
    /// everything else as `Trap::Other` — so captures and usage survive
    /// it. The body touches no frontend value — the handler is only
    /// borrowed (see the trait's safety contract).
    fn invoke(
        &self,
        entry: Entry<'_>,
        frames: Frames<'_>,
        handler: Option<Arc<dyn DispatchHandler>>,
    ) -> Result<Snapshot, Error> {
        let mut store = self.new_store()?;
        if let Some(handler) = handler {
            store.data_mut().bind_on_dispatch(handler);
        }
        let frame_list: Vec<&[u8]> = match &entry {
            Entry::Eval { source } => vec![frames.preamble, source, frames.snippets],
            Entry::Run { .. } => vec![frames.preamble, frames.snippets],
        };
        frames::install_wasi_frames(&mut store, &self.config, &frame_list)?;
        let exports = self.instantiate(&mut store)?;
        let called = match entry {
            Entry::Eval { .. } => {
                let eval = frames::require_export(exports.eval.as_ref())?;
                self.call_with_caps(&mut store, &exports, eval, ())
            }
            Entry::Run { envelope } => {
                let run = frames::require_export(exports.run.as_ref())?;
                let (env_ptr, env_len) = frames::write_envelope(&mut store, &exports, envelope)?;
                self.call_with_caps(&mut store, &exports, run, (env_ptr, env_len))
            }
        };
        let completion = match called {
            Ok(()) => match frames::fetch_outcome_bytes(&mut store, &exports) {
                Ok(bytes) => Completion::Outcome(bytes),
                Err(t) => Completion::Trap(t),
            },
            Err(e) => Completion::Trap(trap::trap_from(e)),
        };
        Ok(self.build_snapshot(&store, completion))
    }
}

/// Drop the memory cap as soon as the guest call returns so that
/// any post-run host bookkeeping (e.g. fetching the OUTCOME_BUFFER,
/// which can grow guest memory transiently) is not attributed to
/// the user script. Also closes the
/// `wall_time` bracket opened by `Driver::prime_caps`. Paired
/// with `Driver::prime_caps`.
fn disarm_caps(store: &mut WtStore<Invocation>) {
    store.data_mut().stop_wall_clock();
    store.data_mut().disarm_memory_cap();
}
