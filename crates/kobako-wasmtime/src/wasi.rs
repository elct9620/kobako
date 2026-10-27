//! The guest's WASI context for one instance: the Preamble on stdin,
//! a capture pipe for each of stdout and stderr, the ambient authority
//! the instance's purpose grants, and a preopen for each Mount an
//! invocation was handed.

use wasmtime_wasi::p1::WasiP1Ctx;
use wasmtime_wasi::p2::pipe::{MemoryInputPipe, MemoryOutputPipe};
use wasmtime_wasi::WasiCtxBuilder;

use crate::ambient::{self, Clocks};
use crate::config::Config;
use crate::descriptors::Descriptors;
use crate::{capture, guest_mem};
use kobako_runtime::error::Trap;
use kobako_runtime::grant::Grant;
use kobako_runtime::mount::Mount;
use kobako_runtime::profile::Profile;
use kobako_transport::abi::FRAME_LEN_SIZE;

/// One instance's WASI context: its descriptor table, the clocks its
/// waits pass on, and the capture pipes stdout and stderr write into, so
/// the captures are read back from the same owner that wired them.
pub(crate) struct Wasi {
    ctx: WasiP1Ctx,
    descriptors: Descriptors,
    clocks: Clocks,
    stdout: MemoryOutputPipe,
    stderr: MemoryOutputPipe,
}

impl Wasi {
    /// An invocation's context, granting the posture the Sandbox requested
    /// and presenting each of the invocation's Mounts as a preopen, from
    /// descriptor 3 in order.
    pub(crate) fn for_invocation(
        config: &Config,
        preamble: &[u8],
        grants: &[Grant],
    ) -> Result<Self, Trap> {
        Self::new(
            preamble,
            config.stdout_limit,
            config.stderr_limit,
            config.open_file_limit,
            config.profile,
            grants.iter().filter_map(to_mount).collect(),
        )
    }

    /// A compile's context, at the strongest posture whatever the Sandbox
    /// requested: compiling the Snippets needs no ambient authority, so
    /// it is granted none.
    pub(crate) fn for_compile(config: &Config, preamble: &[u8]) -> Result<Self, Trap> {
        Self::new(
            preamble,
            config.stdout_limit,
            config.stderr_limit,
            config.open_file_limit,
            Profile::Hermetic,
            Vec::new(),
        )
    }

    /// The ABI version probe's context. Instantiating it runs the
    /// artifact's start section, yet reading a version needs no ambient
    /// authority and no output: the probe takes the strongest posture and
    /// pipes sized to keep nothing.
    pub(crate) fn for_probe() -> Result<Self, Trap> {
        Self::new(&[], Some(0), Some(0), None, Profile::Hermetic, Vec::new())
    }

    /// An uncapped output channel has no ceiling on the host: a guest
    /// rewriting one buffer grows the pipe, not its own memory. The 16 MiB
    /// cap keeps the Preamble's `u32` length prefix from wrapping.
    fn new(
        preamble: &[u8],
        stdout_limit: Option<usize>,
        stderr_limit: Option<usize>,
        open_file_limit: Option<usize>,
        profile: Profile,
        mounts: Vec<Mount>,
    ) -> Result<Self, Trap> {
        // The Preamble carries the same 16 MiB cap as a downcall request
        // (`io::write_request`): past it, the length prefix would silently wrap.
        guest_mem::checked_payload_len(preamble.len())
            .map_err(|msg| Trap::Other(msg.to_string()))?;

        let mut stdin_content: Vec<u8> = Vec::with_capacity(FRAME_LEN_SIZE + preamble.len());
        stdin_content.extend_from_slice(&(preamble.len() as u32).to_be_bytes());
        stdin_content.extend_from_slice(preamble);

        let stdout = MemoryOutputPipe::new(capture::pipe_capacity(stdout_limit));
        let stderr = MemoryOutputPipe::new(capture::pipe_capacity(stderr_limit));

        // stdio lives in kobako's descriptor table, which answers every
        // call that reads a descriptor (`crate::preview1`); the builder
        // keeps only what the remaining imports read.
        let descriptors = Descriptors::new(
            MemoryInputPipe::new(stdin_content),
            stdout.clone(),
            stderr.clone(),
            mounts,
            open_file_limit,
            profile,
        );

        let mut builder = WasiCtxBuilder::new();
        // The profile decides the ambient authority granted: the
        // hermetic rung denies the preview1 time and entropy imports (see
        // `ambient`), the permissive rung leaves the live WASI sources.
        // Ambient filesystem, environment, and network stay absent on both rungs —
        // the builder grants none unless asked. The exhaustive match makes
        // a future ladder rung a compile error here, not authority granted
        // silently.
        let clocks = match profile {
            Profile::Hermetic => {
                builder.wall_clock(ambient::FrozenWallClock);
                builder.monotonic_clock(ambient::FrozenMonotonicClock);
                builder.secure_random(ambient::deterministic_rng());
                Clocks::Frozen
            }
            Profile::Permissive => {
                let monotonic = ambient::LiveMonotonicClock::new();
                builder.monotonic_clock(monotonic);
                Clocks::Live(monotonic)
            }
        };

        Ok(Self {
            ctx: builder.build_p1(),
            descriptors,
            clocks,
            stdout,
            stderr,
        })
    }

    pub(crate) fn ctx_mut(&mut self) -> &mut WasiP1Ctx {
        &mut self.ctx
    }

    pub(crate) fn descriptors(&self) -> &Descriptors {
        &self.descriptors
    }

    pub(crate) fn descriptors_mut(&mut self) -> &mut Descriptors {
        &mut self.descriptors
    }

    pub(crate) fn clocks(&self) -> Clocks {
        self.clocks
    }

    pub(crate) fn stdout_bytes(&self) -> Vec<u8> {
        self.stdout.contents().to_vec()
    }

    pub(crate) fn stderr_bytes(&self) -> Vec<u8> {
        self.stderr.contents().to_vec()
    }
}

/// A Mount as this invocation presents it. A kind this driver does not
/// support has no descriptor to stand behind; `Driver::supports` keeps a
/// frontend from handing one over.
fn to_mount(grant: &Grant) -> Option<Mount> {
    match grant {
        Grant::Mount(mount) => Some(mount.clone()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    //! Witness what an invocation's context grants at each profile rung:
    //! `wasi:clocks` / `wasi:random` frozen under `Hermetic` and live
    //! under `Permissive`, and no ambient filesystem, environment, or
    //! socket under either.
    use wasmtime::{AsContextMut, Module, Store};

    use super::*;
    use crate::cache::shared_engine;
    use crate::imports;
    use crate::invocation::Invocation;

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

    /// Instantiate the probe over an invocation's WASI context built at
    /// `profile` and return a reader for its exports.
    fn probe(profile: Profile) -> impl FnMut(&str) -> i64 {
        let engine = shared_engine().expect("shared engine must be constructible");
        let config = Config {
            timeout: None,
            setup_timeout: None,
            memory_limit: None,
            stdout_limit: None,
            stderr_limit: None,
            open_file_limit: None,
            profile,
        };
        let wasi = Wasi::for_invocation(&config, &[], &[]).expect("WASI context must build");
        let mut store = Store::new(engine, Invocation::new(None, wasi));
        store.set_epoch_deadline(crate::trap::NO_TIMEOUT_EPOCH_DELTA);

        let linker = imports::linker().expect("host imports must link");
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

    /// Assert that the context built at `profile` grants no ambient
    /// directory, environment variable, or socket.
    fn assert_grants_no_ambient_resource(profile: Profile) {
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
    fn hermetic_grants_no_ambient_resource() {
        assert_grants_no_ambient_resource(Profile::Hermetic);
    }

    // @behavior RT-067
    #[test]
    fn permissive_grants_no_ambient_resource() {
        assert_grants_no_ambient_resource(Profile::Permissive);
    }
}
