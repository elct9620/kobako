//! The wasmtime driver: everything needed to run one guest invocation,
//! expressed purely in contract and wasmtime types.
//!
//! A `Driver` is the engine half of a kobako host — the pre-linked
//! `InstancePre` plus the per-Driver caps — and implements the contract
//! `Runtime` trait over it. It is free of any frontend type; a frontend
//! shell (the Ruby ext's `Kobako::Runtime`) only shuttles its
//! host-language values across the contract boundary.

use std::path::Path;
use std::sync::Arc;

use wasmtime::{
    AsContextMut, InstancePre as WtInstancePre, ResourceLimiter, Store as WtStore, TypedFunc,
};

use crate::cache::shared_engine;
use crate::config::Config;
use crate::exports::{self, Exports};
use crate::guest::Guest;
use crate::invocation::Invocation;
use crate::wasi::Wasi;
use crate::{instance_pre, trap};
use kobako_runtime::dispatch::DispatchHandler;
use kobako_runtime::endpoint;
use kobako_runtime::error::{InvokeError, SetupError, Trap};
use kobako_runtime::grant::{Grant, GrantKind};
use kobako_runtime::profile::Profile;
use kobako_runtime::runtime::{Entry, Runtime as ContractRuntime};
use kobako_runtime::snapshot::{Capture, Completion, Snapshot, Usage};
use kobako_transport::envelope::{Downcall, Preamble};

/// The wasmtime execution unit behind one sandbox runtime.
pub struct Driver {
    // Pre-linked instantiation template (import wiring, type checks,
    // and the ABI version check done once in
    // `instance_pre::cached_instance_pre`). Every invocation
    // instantiates a fresh instance from it and discards the whole
    // Store afterwards — the per-invocation instance discipline.
    instance_pre: WtInstancePre<Invocation>,
    // Every cap forwarded from the Sandbox; see `Config`.
    config: Config,
}

impl Driver {
    /// Load the Guest Binary at `path` and verify its ABI version. The
    /// Engine is shared by the process and the Module cached per path;
    /// neither leaves the driver.
    pub fn new(path: &Path, config: Config) -> Result<Self, SetupError> {
        Ok(Self {
            instance_pre: instance_pre::cached_instance_pre(path, config.setup_timeout)?,
            config,
        })
    }

    /// The deadline and the `wall_time` bracket open with the Store, so
    /// instantiating the guest and writing its request count against the
    /// `timeout` as the export call does.
    fn new_store(&self, wasi: Wasi) -> Result<WtStore<Invocation>, SetupError> {
        let mut store = WtStore::new(
            shared_engine()?,
            Invocation::new(self.config.memory_limit, wasi),
        );
        store.limiter(|state: &mut Invocation| -> &mut dyn ResourceLimiter { state.limiter_mut() });
        trap::arm_deadline(&mut store, self.config.timeout);
        store.data_mut().start_wall_clock();
        Ok(store)
    }

    /// Failing here is an engine fault — a `Trap` — unlike the
    /// construction-time probe, whose failure is a `SetupError`.
    fn instantiate(&self, store: &mut WtStore<Invocation>) -> Result<Exports, Trap> {
        let instance = self
            .instance_pre
            .instantiate(store.as_context_mut())
            .map_err(|e| {
                trap::cap_trap(&e).unwrap_or_else(|| {
                    Trap::Other(format!("failed to instantiate the Sandbox runtime: {e}"))
                })
            })?;
        let exports = Exports::resolve(&instance, store.as_context_mut());
        store.data_mut().set_exports(exports.clone());
        Ok(exports)
    }

    /// Disarm runs whether the call returns or traps, so the `wall_time`
    /// bracket and the memory cap always close; that guarantee is why the
    /// bracket lives in one place rather than at each call site.
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
        self.arm_memory_cap(store, exports);
        let result = export.call(store.as_context_mut(), params);
        disarm_caps(store);
        result
    }

    /// The pre-initialized image's allocation is folded into the memory
    /// baseline rather than the budget, so the cap arms only once the
    /// image is in place.
    fn arm_memory_cap(&self, store: &mut WtStore<Invocation>, exports: &Exports) {
        let baseline = match exports.memory {
            Some(m) => m.data_size(store.as_context_mut()),
            None => 0,
        };
        store.data_mut().arm_memory_cap(baseline);
    }

    /// Built the same for every `completion`: captures and usage must
    /// survive a trap just as they do an outcome.
    fn build_snapshot(&self, store: &WtStore<Invocation>, completion: Completion) -> Snapshot {
        let data = store.data();
        let mut usage = Usage::default();
        usage.wall_time = data.wall_time().as_secs_f64();
        usage.memory_peak = data.memory_peak();
        let clip = |bytes, limit| {
            let mut capture = Capture::default();
            (capture.bytes, capture.truncated) = endpoint::clip_capture(bytes, limit);
            capture
        };
        Snapshot::new(
            completion,
            clip(data.wasi().stdout_bytes(), self.config.stdout_limit),
            clip(data.wasi().stderr_bytes(), self.config.stderr_limit),
            usage,
        )
    }
}

impl ContractRuntime for Driver {
    /// A fault before the export call is the `Err`; once the call
    /// starts, every fault folds into the Snapshot's `Completion` so
    /// captures and usage survive it. The handler is only borrowed (see the
    /// trait's safety contract).
    fn invoke(
        &self,
        entry: Entry<'_>,
        preamble: &Preamble,
        handler: Option<Arc<dyn DispatchHandler>>,
        grants: &[Grant],
    ) -> Result<Snapshot, InvokeError> {
        let wasi = Wasi::for_invocation(&self.config, &preamble.encode(), grants)?;
        let mut store = self.new_store(wasi)?;
        if let Some(handler) = handler {
            store.data_mut().bind_on_dispatch(handler);
        }
        let exports = self.instantiate(&mut store)?;
        let downcall = exports::require(exports.downcall.as_ref())?;
        let request = endpoint::request_of(entry);
        let (ptr, len) = endpoint::write_request(&mut guest_of(&mut store), &request)?;
        let called = self.call_with_caps(&mut store, &exports, downcall, (ptr, len));
        let completion = match called {
            Ok(packed) => match endpoint::read_outcome(&mut guest_of(&mut store), packed) {
                Ok(outcome) => Completion::Outcome(outcome),
                Err(t) => Completion::Trap(t),
            },
            Err(e) => Completion::Trap(trap::to_trap(e)),
        };
        Ok(self.build_snapshot(&store, completion))
    }

    /// Exactly the rung `Config` requested. `Hermetic` freezes ambient time
    /// and entropy; both rungs wire no ambient filesystem, environment, or network,
    /// and the linker adds only `__kobako_upcall` beyond that WASI surface.
    fn profile(&self) -> Profile {
        self.config.profile
    }

    /// A Mount becomes a preopen of kobako's own descriptor table, which
    /// answers every preview1 call beneath it.
    fn supports(&self, kind: GrantKind) -> bool {
        matches!(kind, GrantKind::Mount)
    }

    /// Bounded by the same caps as an invocation, on an instance of its
    /// own that no script ever runs on, so what it answers carries
    /// nothing a script could have written.
    fn compile(&self, preamble: &Preamble) -> Option<Vec<Vec<u8>>> {
        let wasi = Wasi::for_compile(&self.config, &preamble.encode()).ok()?;
        let mut store = self.new_store(wasi).ok()?;
        let exports = self.instantiate(&mut store).ok()?;
        let downcall = exports::require(exports.downcall.as_ref()).ok()?;
        let (ptr, len) =
            endpoint::write_request(&mut guest_of(&mut store), &Downcall::Compile.encode()).ok()?;
        let packed = self
            .call_with_caps(&mut store, &exports, downcall, (ptr, len))
            .ok()?;
        let reply = endpoint::read_compile_reply(&mut guest_of(&mut store), packed).ok()?;
        endpoint::compiled_bytecodes(preamble, reply)
    }
}

/// The instance the runtime's host ABI endpoint drives, through `store`.
fn guest_of(store: &mut WtStore<Invocation>) -> Guest<'_> {
    Guest(store.as_context_mut())
}

fn disarm_caps(store: &mut WtStore<Invocation>) {
    store.data_mut().stop_wall_clock();
    store.data_mut().disarm_memory_cap();
}
