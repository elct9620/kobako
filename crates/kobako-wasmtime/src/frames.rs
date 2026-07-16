//! Per-invocation byte-shuttle between the host and guest linear memory:
//! it resolves the required `memory` / ABI-export handles, writes the
//! `#run` envelope into a freshly allocated guest buffer, builds the
//! stdin frame stream plus stdout / stderr capture pipes for the WASI
//! context, and reads the OUTCOME_BUFFER back out. The driver owns no
//! wire codec — these helpers move raw bytes; the frontend decodes them.

use wasmtime::{AsContextMut, Memory, Store as WtStore, TypedFunc};
use wasmtime_wasi::p2::pipe::{MemoryInputPipe, MemoryOutputPipe};
use wasmtime_wasi::WasiCtxBuilder;

use crate::config::Config;
use crate::exports::Exports;
use crate::invocation::Invocation;
use crate::{ambient, capture, guest_mem};
use kobako_runtime::error::{Error, SetupError, Trap};
use kobako_runtime::profile::Profile;

/// Return the resolved `memory` export handle, or a `Trap` when the loaded
/// module exports no linear memory — the "not a Kobako-shaped runtime"
/// failure mode (`guest_mem::SANDBOX_RUNTIME_NOT_KOBAKO`).
fn require_memory(exports: &Exports) -> Result<Memory, Trap> {
    exports
        .memory
        .ok_or_else(|| Trap::Other(guest_mem::SANDBOX_RUNTIME_NOT_KOBAKO.to_string()))
}

/// Allocate a `len`-byte buffer in guest linear memory via
/// `__kobako_alloc`, copy `envelope` into it, and return `(ptr, len)`
/// as `i32` values matching the `__kobako_run(env_ptr, env_len)` ABI.
/// Returns a `Trap` when the allocation hook is missing or itself traps
/// (an engine fault), and a runtime-intact `SetupError` when the hook runs
/// but cannot reserve the buffer (`__kobako_alloc` returns 0). The ext
/// boundary maps these to `Kobako::TrapError` / `Kobako::SandboxError`.
pub(crate) fn write_envelope(
    store: &mut WtStore<Invocation>,
    exports: &Exports,
    envelope: &[u8],
) -> Result<(i32, i32), Error> {
    let len_i32 = guest_mem::checked_payload_len(envelope.len())
        .map_err(|msg| Trap::Other(msg.to_string()))?;

    let alloc = require_export(exports.alloc.as_ref())?;
    let memory = require_memory(exports)?;

    let ptr = alloc
        .call(store.as_context_mut(), len_i32 as u32)
        .map_err(|e| Trap::Other(format!("failed to allocate input buffer: {e}")))?;
    if ptr == 0 {
        return Err(SetupError::Intact(
            "could not allocate input buffer (out of memory)".to_string(),
        )
        .into());
    }
    let data = memory.data_mut(store.as_context_mut());
    let range = guest_mem::guest_buffer_range(ptr as usize, envelope.len(), data.len())
        .map_err(|msg| Trap::Other(msg.to_string()))?;
    data[range].copy_from_slice(envelope);

    Ok((ptr as i32, len_i32))
}

/// Build the per-invocation WASI context with stdin carrying every frame
/// in `frames` (each prefixed by its 4-byte big-endian u32 length —
/// docs/wire-codec.md § Invocation channels) plus fresh stdout / stderr
/// pipes, and install it on the invocation's Store. `#eval` passes three
/// frames (preamble, source, snippets), `#run` passes two (preamble,
/// snippets — the invocation envelope arrives via linear memory
/// instead). Each output pipe is sized at `cap + 1` so
/// `capture::clip_capture` can distinguish "wrote exactly cap bytes"
/// from "exceeded cap"; uncapped channels fall back to `usize::MAX` and
/// rely on `memory_limit` for the real ceiling.
/// Returns a `Trap` when any frame exceeds the 16 MiB cap that keeps its
/// `u32` length prefix from wrapping (boundary → `Kobako::TrapError`).
pub(crate) fn install_wasi_frames(
    store: &mut WtStore<Invocation>,
    config: &Config,
    frames: &[&[u8]],
) -> Result<(), Trap> {
    // Every frame carries the same 16 MiB cap as the `#run` envelope
    // (`write_envelope`): the length prefix is a `u32`, so a frame past
    // the cap would silently wrap and corrupt the stdin frame stream.
    for &frame in frames {
        guest_mem::checked_payload_len(frame.len()).map_err(|msg| Trap::Other(msg.to_string()))?;
    }

    let total: usize = frames.iter().map(|&f| 4 + f.len()).sum();
    let mut stdin_content: Vec<u8> = Vec::with_capacity(total);
    for &frame in frames {
        stdin_content.extend_from_slice(&(frame.len() as u32).to_be_bytes());
        stdin_content.extend_from_slice(frame);
    }

    let stdin_pipe = MemoryInputPipe::new(stdin_content);
    let stdout_pipe = MemoryOutputPipe::new(capture::pipe_capacity(config.stdout_limit_bytes));
    let stderr_pipe = MemoryOutputPipe::new(capture::pipe_capacity(config.stderr_limit_bytes));

    let mut builder = WasiCtxBuilder::new();
    builder.stdin(stdin_pipe);
    builder.stdout(stdout_pipe.clone());
    builder.stderr(stderr_pipe.clone());
    // The requested profile decides the ambient-authority grant: the
    // hermetic rung denies the preview1 time and entropy imports (see
    // `ambient`), the permissive rung leaves the live WASI sources.
    // Filesystem, environment, and network stay absent on both rungs —
    // the builder grants none unless asked. The exhaustive match makes
    // a future ladder rung a compile error here, not a silent grant.
    match config.profile {
        Profile::Hermetic => {
            builder.wall_clock(ambient::FrozenWallClock);
            builder.monotonic_clock(ambient::FrozenMonotonicClock);
            builder.secure_random(ambient::deterministic_rng());
        }
        Profile::Permissive => {}
    }
    let wasi = builder.build_p1();

    store
        .data_mut()
        .install_wasi(wasi, stdout_pipe, stderr_pipe);
    Ok(())
}

/// Invoke `__kobako_take_outcome`, decode the packed `(ptr<<32)|len`
/// u64, and copy the OUTCOME_BUFFER slice out of guest memory. Returns a
/// `Trap` (boundary → `Kobako::TrapError`) when the export is missing,
/// `len` exceeds the 16 MiB single-dispatch cap, the `ptr`/`len`
/// arithmetic overflows, the slice falls outside live memory, or the
/// `memory` export itself is absent.
pub(crate) fn fetch_outcome_bytes(
    store: &mut WtStore<Invocation>,
    exports: &Exports,
) -> Result<Vec<u8>, Trap> {
    let take = require_export(exports.take_outcome.as_ref())?;
    let mem = require_memory(exports)?;

    let packed = take
        .call(store.as_context_mut(), ())
        .map_err(|e| Trap::Other(format!("failed to read the Sandbox result: {e}")))?;
    let (ptr, len) = guest_mem::unpack_outcome_packed(packed);
    if len > guest_mem::MAX_DISPATCH_PAYLOAD {
        return Err(Trap::Other(
            "result payload exceeds the 16 MiB limit".to_string(),
        ));
    }

    let data = mem.data(store.as_context_mut());
    let range = guest_mem::guest_buffer_range(ptr, len, data.len())
        .map_err(|msg| Trap::Other(format!("the Sandbox result is out of bounds: {msg}")))?;
    Ok(data[range].to_vec())
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

/// Return the resolved `TypedFunc` for an ABI export, or a `Trap`
/// (boundary → `Kobako::TrapError`) when the option is `None`. Both
/// run-path methods (`#eval`, `#run`) plus the `build_snapshot` readout
/// that drains `OUTCOME_BUFFER` share the same "missing export" handling;
/// this helper collapses those sites onto one safe entry. The user-facing
/// message is intentionally export-agnostic (see
/// `SANDBOX_RUNTIME_MISSING_HOOKS`) — the ABI symbol name is not
/// actionable to callers, so it is not threaded in.
pub(crate) fn require_export<Params, Results>(
    export: Option<&TypedFunc<Params, Results>>,
) -> Result<&TypedFunc<Params, Results>, Trap>
where
    Params: wasmtime::WasmParams,
    Results: wasmtime::WasmResults,
{
    export.ok_or_else(|| Trap::Other(SANDBOX_RUNTIME_MISSING_HOOKS.to_string()))
}

#[cfg(test)]
mod tests {
    //! Witness the B-54 rung split at the WASI boundary: one probe
    //! module reads `wasi:clocks` / `wasi:random` through the context
    //! `install_wasi_frames` builds — frozen under `Hermetic`, live
    //! under `Permissive`.
    use wasmtime::{Linker, Module};
    use wasmtime_wasi::p1;

    use super::*;
    use crate::cache::shared_engine;

    /// Preview1 probe with one export per ambient source: `clock_ns`
    /// reads the realtime clock, `random_word` reads eight entropy bytes.
    const AMBIENT_PROBE_WAT: &str = r#"
        (module
          (import "wasi_snapshot_preview1" "clock_time_get"
            (func $clock (param i32 i64 i32) (result i32)))
          (import "wasi_snapshot_preview1" "random_get"
            (func $random (param i32 i32) (result i32)))
          (memory (export "memory") 1)
          (func (export "clock_ns") (result i64)
            (drop (call $clock (i32.const 0) (i64.const 1) (i32.const 0)))
            (i64.load (i32.const 0)))
          (func (export "random_word") (result i64)
            (drop (call $random (i32.const 8) (i32.const 8)))
            (i64.load (i32.const 8))))
    "#;

    /// Instantiate the probe over a WASI context built at `profile` and
    /// return the `(clock, random)` readings it observes.
    fn probe_ambient(profile: Profile) -> (i64, i64) {
        let engine = shared_engine().expect("shared engine must be constructible");
        let config = Config {
            timeout: None,
            stdout_limit_bytes: None,
            stderr_limit_bytes: None,
            profile,
        };
        let mut store = WtStore::new(engine, Invocation::new(None));
        store.set_epoch_deadline(crate::trap::NO_TIMEOUT_EPOCH_DELTA);
        install_wasi_frames(&mut store, &config, &[]).expect("WASI context must install");

        let mut linker: Linker<Invocation> = Linker::new(engine);
        p1::add_to_linker_sync(&mut linker, |state: &mut Invocation| state.wasi_mut())
            .expect("WASI imports must link");
        let module = Module::new(engine, AMBIENT_PROBE_WAT).expect("probe module must compile");
        let instance = linker
            .instantiate(&mut store, &module)
            .expect("probe module must instantiate");

        let read = |store: &mut WtStore<Invocation>, name: &str| -> i64 {
            instance
                .get_typed_func::<(), i64>(store.as_context_mut(), name)
                .expect("probe export must resolve")
                .call(store.as_context_mut(), ())
                .expect("probe export must run")
        };
        let clock = read(&mut store, "clock_ns");
        let random = read(&mut store, "random_word");
        (clock, random)
    }

    #[test]
    fn hermetic_denies_ambient_time_and_entropy() {
        let (clock, random) = probe_ambient(Profile::Hermetic);
        assert_eq!(
            clock, 0,
            "a hermetic guest's wasi:clocks must read the Unix epoch, not host time"
        );
        assert_eq!(
            random, 0,
            "a hermetic guest's wasi:random must yield the constant stream, not host entropy"
        );
    }

    #[test]
    fn permissive_grants_live_ambient_time_and_entropy() {
        let (clock, random) = probe_ambient(Profile::Permissive);
        assert!(
            clock > 0,
            "a permissive guest's wasi:clocks must read live host time"
        );
        assert_ne!(
            random, 0,
            "a permissive guest's wasi:random must yield host entropy"
        );
    }
}
