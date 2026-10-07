//! Per-invocation byte-shuttle between the host and guest linear memory:
//! it resolves the required `memory` / ABI-export handles, writes the
//! `#run` envelope into a freshly allocated guest buffer, builds the
//! stdin frame stream plus stdout / stderr capture pipes for the WASI
//! context, and reads the OUTCOME_BUFFER back out. The driver owns no
//! wire codec — these helpers move raw bytes; the frontend decodes them.

use wasmtime::{AsContextMut, Store as WtStore};
use wasmtime_wasi::p2::pipe::{MemoryInputPipe, MemoryOutputPipe};
use wasmtime_wasi::WasiCtxBuilder;

use crate::config::Config;
use crate::exports::{self, Exports};
use crate::invocation::Invocation;
use crate::{ambient, capture, guest_mem};
use kobako_runtime::error::{InvokeError, SetupError, Trap};
use kobako_runtime::profile::Profile;
use kobako_transport::abi::FRAME_LEN_SIZE;

/// A missing or trapping allocator is an engine fault, a `Trap`; one that
/// runs but returns 0 leaves the runtime intact, so it is a `SetupError`.
pub(crate) fn write_envelope(
    store: &mut WtStore<Invocation>,
    exports: &Exports,
    envelope: &[u8],
) -> Result<(i32, i32), InvokeError> {
    let len_i32 = guest_mem::checked_payload_len(envelope.len())
        .map_err(|msg| Trap::Other(msg.to_string()))?;

    let alloc = exports::require(exports.alloc.as_ref())?;
    let memory = exports.require_memory()?;

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

/// An uncapped output channel relies on `memory_limit` for its real
/// ceiling. The 16 MiB frame cap keeps each `u32` length prefix from
/// wrapping.
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

    let total: usize = frames.iter().map(|&f| FRAME_LEN_SIZE + f.len()).sum();
    let mut stdin_content: Vec<u8> = Vec::with_capacity(total);
    for &frame in frames {
        stdin_content.extend_from_slice(&(frame.len() as u32).to_be_bytes());
        stdin_content.extend_from_slice(frame);
    }

    let stdin_pipe = MemoryInputPipe::new(stdin_content);
    let stdout_pipe = MemoryOutputPipe::new(capture::pipe_capacity(config.stdout_limit));
    let stderr_pipe = MemoryOutputPipe::new(capture::pipe_capacity(config.stderr_limit));

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

pub(crate) fn fetch_outcome_bytes(
    store: &mut WtStore<Invocation>,
    exports: &Exports,
) -> Result<Vec<u8>, Trap> {
    let take = exports::require(exports.take_outcome.as_ref())?;
    let mem = exports.require_memory()?;

    let packed = take
        .call(store.as_context_mut(), ())
        .map_err(|e| Trap::Other(format!("failed to read the Sandbox result: {e}")))?;
    let (ptr, len) = guest_mem::unpack_guest_buffer(packed);
    if len > kobako_transport::abi::MAX_DISPATCH_PAYLOAD {
        return Err(Trap::Other(
            "result payload exceeds the 16 MiB limit".to_string(),
        ));
    }

    let data = mem.data(store.as_context_mut());
    let range = guest_mem::guest_buffer_range(ptr, len, data.len())
        .map_err(|msg| Trap::Other(format!("the Sandbox result is out of bounds: {msg}")))?;
    Ok(data[range].to_vec())
}

#[cfg(test)]
mod tests {
    //! Witness what the context `install_wasi_frames` builds grants at
    //! each profile rung: `wasi:clocks` / `wasi:random` frozen under
    //! `Hermetic` and live under `Permissive`, and no filesystem,
    //! environment, or socket under either.
    use wasmtime::{Linker, Module};
    use wasmtime_wasi::p1;

    use super::*;
    use crate::cache::shared_engine;

    /// Preview1 `errno` for a descriptor the context does not hold.
    const ERRNO_BADF: i64 = 8;

    /// Preview1 probe with one export per ambient source: `clock_ns`
    /// reads the realtime clock, `random_word` reads eight entropy bytes,
    /// `environ_count` counts environment variables, and `prestat_errno`
    /// / `accept_errno` ask descriptor 3 for a directory and a
    /// connection. Preview1 numbers preopens from 3, so nothing there
    /// means no directory and no socket was granted.
    const PROBE_WAT: &str = r#"
        (module
          (import "wasi_snapshot_preview1" "clock_time_get"
            (func $clock (param i32 i64 i32) (result i32)))
          (import "wasi_snapshot_preview1" "random_get"
            (func $random (param i32 i32) (result i32)))
          (import "wasi_snapshot_preview1" "environ_sizes_get"
            (func $environ (param i32 i32) (result i32)))
          (import "wasi_snapshot_preview1" "fd_prestat_get"
            (func $prestat (param i32 i32) (result i32)))
          (import "wasi_snapshot_preview1" "sock_accept"
            (func $accept (param i32 i32 i32) (result i32)))
          (memory (export "memory") 1)
          (func (export "clock_ns") (result i64)
            (drop (call $clock (i32.const 0) (i64.const 1) (i32.const 0)))
            (i64.load (i32.const 0)))
          (func (export "random_word") (result i64)
            (drop (call $random (i32.const 8) (i32.const 8)))
            (i64.load (i32.const 8)))
          (func (export "environ_count") (result i64)
            (drop (call $environ (i32.const 16) (i32.const 20)))
            (i64.extend_i32_u (i32.load (i32.const 16))))
          (func (export "prestat_errno") (result i64)
            (i64.extend_i32_u (call $prestat (i32.const 3) (i32.const 24))))
          (func (export "accept_errno") (result i64)
            (i64.extend_i32_u (call $accept (i32.const 3) (i32.const 0) (i32.const 24)))))
    "#;

    /// Instantiate the probe over a WASI context built at `profile` and
    /// return a reader for its exports.
    fn probe(profile: Profile) -> impl FnMut(&str) -> i64 {
        let engine = shared_engine().expect("shared engine must be constructible");
        let config = Config {
            timeout: None,
            memory_limit: None,
            stdout_limit: None,
            stderr_limit: None,
            profile,
        };
        let mut store = WtStore::new(engine, Invocation::new(None));
        store.set_epoch_deadline(crate::trap::NO_TIMEOUT_EPOCH_DELTA);
        install_wasi_frames(&mut store, &config, &[]).expect("WASI context must install");

        let mut linker: Linker<Invocation> = Linker::new(engine);
        p1::add_to_linker_sync(&mut linker, |state: &mut Invocation| state.wasi_mut())
            .expect("WASI imports must link");
        let module = Module::new(engine, PROBE_WAT).expect("probe module must compile");
        let instance = linker
            .instantiate(&mut store, &module)
            .expect("probe module must instantiate");

        move |name| {
            instance
                .get_typed_func::<(), i64>(store.as_context_mut(), name)
                .expect("probe export must resolve")
                .call(store.as_context_mut(), ())
                .expect("probe export must run")
        }
    }

    /// Assert that the context built at `profile` grants no directory,
    /// environment variable, or socket.
    fn assert_grants_no_resource(profile: Profile) {
        let mut read = probe(profile);
        assert_eq!(
            read("prestat_errno"),
            ERRNO_BADF,
            "a {profile:?} guest's WASI context must preopen no directory"
        );
        assert_eq!(
            read("environ_count"),
            0,
            "a {profile:?} guest's WASI context must carry no environment variable"
        );
        assert_eq!(
            read("accept_errno"),
            ERRNO_BADF,
            "a {profile:?} guest's WASI context must hold no socket"
        );
    }

    // @behavior RT-047 RT-048
    #[test]
    fn hermetic_denies_ambient_time_and_entropy() {
        let mut read = probe(Profile::Hermetic);
        assert_eq!(
            read("clock_ns"),
            0,
            "a hermetic guest's wasi:clocks must read the Unix epoch, not host time"
        );
        assert_eq!(
            read("random_word"),
            0,
            "a hermetic guest's wasi:random must yield the constant stream, not host entropy"
        );
    }

    // @behavior RT-049 RT-050
    #[test]
    fn permissive_grants_live_ambient_time_and_entropy() {
        let mut read = probe(Profile::Permissive);
        assert!(
            read("clock_ns") > 0,
            "a permissive guest's wasi:clocks must read live host time"
        );
        assert_ne!(
            read("random_word"),
            0,
            "a permissive guest's wasi:random must yield host entropy"
        );
    }

    // @behavior RT-067
    #[test]
    fn hermetic_grants_no_filesystem_environment_or_socket() {
        assert_grants_no_resource(Profile::Hermetic);
    }

    // @behavior RT-067
    #[test]
    fn permissive_grants_no_filesystem_environment_or_socket() {
        assert_grants_no_resource(Profile::Permissive);
    }
}
