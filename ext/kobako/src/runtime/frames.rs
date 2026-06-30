//! Per-invocation byte-shuttle between Ruby and guest linear memory: it
//! resolves the required `memory` / ABI-export handles, writes the `#run`
//! envelope into a freshly allocated guest buffer, builds the stdin frame
//! stream plus stdout / stderr capture pipes for the WASI context, and
//! reads the OUTCOME_BUFFER back out. The ext owns no wire codec — these
//! helpers move raw bytes; Ruby decodes them.

use wasmtime::{AsContextMut, Memory, Store as WtStore, TypedFunc};
use wasmtime_wasi::p2::pipe::{MemoryInputPipe, MemoryOutputPipe};
use wasmtime_wasi::WasiCtxBuilder;

use super::config::Config;
use super::exports::Exports;
use super::invocation::Invocation;
use super::{ambient, capture, guest_mem};
use crate::contract::error::{Error, SetupError, Trap};

/// Return the resolved `memory` export handle, or a `Trap` when the loaded
/// module exports no linear memory — the "not a Kobako-shaped runtime"
/// failure mode (`SANDBOX_RUNTIME_NOT_KOBAKO`).
fn require_memory(exports: &Exports) -> Result<Memory, Trap> {
    exports
        .memory
        .ok_or_else(|| Trap::Other(SANDBOX_RUNTIME_NOT_KOBAKO.to_string()))
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
        .call(store.as_context_mut(), envelope.len() as u32)
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
    // Deny the preview1 ambient-authority imports the guest never legitimately
    // reaches but the WASI layer would otherwise grant (see `ambient`).
    builder.wall_clock(ambient::FrozenWallClock);
    builder.monotonic_clock(ambient::FrozenMonotonicClock);
    builder.secure_random(ambient::deterministic_rng());
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

/// User-facing message for the "the loaded Wasm module is not a
/// Kobako-shaped runtime at all" failure mode (no linear memory
/// export). Same phrasing philosophy as
/// `SANDBOX_RUNTIME_MISSING_HOOKS`.
const SANDBOX_RUNTIME_NOT_KOBAKO: &str =
    "the loaded Wasm module is not a Kobako-compatible runtime";

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
