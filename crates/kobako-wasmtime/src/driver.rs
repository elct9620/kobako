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
use std::time::Instant;

use wasmtime::{
    AsContextMut, InstancePre as WtInstancePre, ResourceLimiter, Store as WtStore, TypedFunc,
};

use crate::cache::shared_engine;
use crate::config::Config;
use crate::exports::{self, Exports};
use crate::invocation::Invocation;
use crate::wasi::Wasi;
use crate::{capture, guest_mem, instance_pre, io, trap};
use kobako_runtime::dispatch::DispatchHandler;
use kobako_runtime::error::{InvokeError, SetupError, Trap};
use kobako_runtime::grant::{Grant, GrantKind};
use kobako_runtime::profile::Profile;
use kobako_runtime::runtime::{Entry, Runtime as ContractRuntime};
use kobako_runtime::snapshot::{Capture, Completion, Snapshot, Usage};
use kobako_transport::envelope::{CompileReply, Downcall, Preamble, Run, Snippet};

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
            instance_pre: instance_pre::cached_instance_pre(path)?,
            config,
        })
    }

    fn new_store(&self, wasi: Wasi) -> Result<WtStore<Invocation>, SetupError> {
        let mut store = WtStore::new(
            shared_engine()?,
            Invocation::new(self.config.memory_limit, wasi),
        );
        store.limiter(|state: &mut Invocation| -> &mut dyn ResourceLimiter { state.limiter_mut() });
        store.epoch_deadline_callback(trap::epoch_deadline_callback);
        Ok(store)
    }

    /// Failing here is an engine fault — a `Trap` — unlike the
    /// construction-time probe, whose failure is a `SetupError`.
    fn instantiate(&self, store: &mut WtStore<Invocation>) -> Result<Exports, Trap> {
        let instance = self
            .instance_pre
            .instantiate(store.as_context_mut())
            .map_err(|e| Trap::Other(format!("failed to instantiate the Sandbox runtime: {e}")))?;
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
        self.prime_caps(store, exports);
        let result = export.call(store.as_context_mut(), params);
        disarm_caps(store);
        result
    }

    /// The pre-initialized image's allocation is folded into the memory
    /// baseline rather than the budget.
    fn prime_caps(&self, store: &mut WtStore<Invocation>, exports: &Exports) {
        match self.config.timeout {
            Some(timeout) => {
                let deadline = Instant::now() + timeout;
                store.data_mut().set_deadline(Some(deadline));
                store.set_epoch_deadline(1);
            }
            None => {
                store.data_mut().set_deadline(None);
                store.set_epoch_deadline(trap::NO_TIMEOUT_EPOCH_DELTA);
            }
        }
        let baseline = match exports.memory {
            Some(m) => m.data_size(store.as_context_mut()),
            None => 0,
        };
        store.data_mut().arm_memory_cap(baseline);
        store.data_mut().start_wall_clock();
    }

    /// Built the same for every `completion`: captures and usage must
    /// survive a trap just as they do an outcome.
    fn build_snapshot(&self, store: &WtStore<Invocation>, completion: Completion) -> Snapshot {
        let data = store.data();
        let usage = Usage {
            wall_time: data.wall_time().as_secs_f64(),
            memory_peak: data.memory_peak(),
        };
        let (stdout_bytes, stdout_truncated) =
            capture::clip_capture(data.wasi().stdout_bytes(), self.config.stdout_limit);
        let (stderr_bytes, stderr_truncated) =
            capture::clip_capture(data.wasi().stderr_bytes(), self.config.stderr_limit);
        Snapshot {
            completion,
            stdout: Capture {
                bytes: stdout_bytes,
                truncated: stdout_truncated,
            },
            stderr: Capture {
                bytes: stderr_bytes,
                truncated: stderr_truncated,
            },
            usage,
        }
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
        let request = match entry {
            Entry::Eval { source } => Downcall::Eval(source.to_vec()),
            Entry::Run {
                entrypoint,
                payload,
            } => Downcall::Run(Run {
                entrypoint: entrypoint.to_owned(),
                payload,
            }),
        }
        .encode();
        let (ptr, len) = io::write_request(&mut store, &exports, &request)?;
        let called = self.call_with_caps(&mut store, &exports, downcall, (ptr, len));
        let completion = match called {
            Ok(packed) => match io::read_outcome(&store, &exports, packed) {
                Ok(outcome) => Completion::Outcome(outcome),
                Err(t) => Completion::Trap(t),
            },
            Err(e) => Completion::Trap(trap::trap_from(e)),
        };
        Ok(self.build_snapshot(&store, completion))
    }

    /// Exactly the rung `Config` requested. `Hermetic` freezes ambient time
    /// and entropy; both rungs wire no ambient filesystem, environment, or network,
    /// and the linker adds only `__kobako_upcall` beyond that WASI surface.
    fn profile(&self) -> Profile {
        self.config.profile
    }

    /// A filesystem Grant becomes a preopen of kobako's own descriptor
    /// table, which answers every preview1 call beneath it.
    fn supports(&self, kind: GrantKind) -> bool {
        matches!(kind, GrantKind::Filesystem)
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
            io::write_request(&mut store, &exports, &Downcall::Compile.encode()).ok()?;
        let packed = self
            .call_with_caps(&mut store, &exports, downcall, (ptr, len))
            .ok()?;
        match io::read_compile_reply(&store, &exports, packed).ok()? {
            CompileReply::Ok(bytecodes) => one_per_source(preamble, bytecodes)
                .filter(|bytecodes| replay_fits(preamble, bytecodes)),
            CompileReply::Error(_) | CompileReply::Unsupported(_) => None,
        }
    }
}

/// Each invocation sends the Preamble with every bytecode in place of its
/// source, so an answer that would carry it past the payload cap would trap
/// every invocation the source alone fits.
fn replay_fits(preamble: &Preamble, bytecodes: &[Vec<u8>]) -> bool {
    let mut replay = preamble.clone();
    let sources = replay
        .snippets
        .entries
        .iter_mut()
        .filter(|entry| matches!(entry, Snippet::Source { .. }));
    for (entry, body) in sources.zip(bytecodes) {
        *entry = Snippet::Bytecode { body: body.clone() };
    }
    guest_mem::checked_payload_len(replay.encode().len()).is_ok()
}

/// A frontend replays each bytecode in place of one source snippet, in
/// order, so an answer of any other length leaves nothing to replay.
fn one_per_source(preamble: &Preamble, bytecodes: Vec<Vec<u8>>) -> Option<Vec<Vec<u8>>> {
    let sources = preamble
        .snippets
        .entries
        .iter()
        .filter(|entry| matches!(entry, Snippet::Source { .. }))
        .count();
    (bytecodes.len() == sources).then_some(bytecodes)
}

fn disarm_caps(store: &mut WtStore<Invocation>) {
    store.data_mut().stop_wall_clock();
    store.data_mut().disarm_memory_cap();
}

#[cfg(test)]
mod tests {
    use kobako_transport::envelope::Snippets;

    use super::*;

    /// One source between two bytecode snippets: a compile answers one
    /// bytecode for it.
    fn one_source() -> Preamble {
        let mut preamble = Preamble::default();
        preamble.snippets = Snippets {
            entries: vec![
                Snippet::Bytecode { body: vec![0xaa] },
                Snippet::Source {
                    name: "Only".into(),
                    body: "1".into(),
                },
                Snippet::Bytecode { body: vec![0xbb] },
            ],
        };
        preamble
    }

    // @behavior WE-089
    #[test]
    fn a_compiled_answer_of_another_length_leaves_nothing_compiled() {
        let preamble = one_source();

        assert_eq!(
            [
                one_per_source(&preamble, Vec::new()),
                one_per_source(&preamble, vec![vec![0x01], vec![0x02]]),
                one_per_source(&preamble, vec![vec![0x01]]),
            ],
            [None, None, Some(vec![vec![0x01]])],
            "a compiled answer through Driver#compile must be kept only when it carries one \
             bytecode per source snippet"
        );
    }
}
