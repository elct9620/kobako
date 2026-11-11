//! The guest's WASI context for one instance: the Preamble on stdin,
//! a capture pipe for each of stdout and stderr, the ambient authority
//! the instance's purpose grants, a preopen for each Mount an invocation
//! was handed, and the preview2 resources it holds, with the network its
//! network Grants open.

use wasmtime_wasi::p1::WasiP1Ctx;
use wasmtime_wasi::WasiCtxBuilder;

use crate::ambient::{self, Clocks};
use crate::config::Config;
use crate::descriptors::Descriptors;
use crate::preview2;
use kobako_runtime::endpoint;
use kobako_runtime::error::Trap;
use std::sync::Arc;

use kobako_runtime::filesystem::{create_pipe_files, PipeInodeInfo};
use kobako_runtime::grant::Grant;
use kobako_runtime::mount::Mount;
use kobako_runtime::net::{HostInet, HyperOutbound, Namespace, OpenConnections};
use kobako_runtime::profile::Profile;
use kobako_runtime::security::prepare_creds_within;
use kobako_runtime::vfs::{Cred, OpenFiles};

/// The Cred an invocation's Mounts open as: kobako's own rules, then each
/// module the Driver was built with, its Filesystems holding at most the
/// memory the Driver allots, as Linux's `prepare_creds` gives a task the
/// LSMs it booted with.
fn prepare_creds(config: &Config) -> Arc<Cred> {
    prepare_creds_within(&config.security, config.memory_limit)
}

/// The most entries preview1's descriptor table or preview2's resource table
/// holds, as wasmtime's `ResourceTable::max_capacity` bounds a table.
pub(crate) const MAX_TABLE_ENTRIES: usize = 1_000_000;

/// One instance's WASI context: who its calls are made for, each encoding's
/// table, the clocks its waits pass on, and the capture pipes stdout and
/// stderr write into, so the captures are read back from the same owner that
/// wired them.
pub(crate) struct Wasi {
    ctx: WasiP1Ctx,
    /// Who the guest's calls are made for, through either encoding, as a
    /// task holds its `cred`.
    cred: Arc<Cred>,
    /// The isolation the guest runs under, through either encoding.
    profile: Profile,
    descriptors: Descriptors,
    table: preview2::Table,
    clocks: Clocks,
    stdout: Arc<PipeInodeInfo>,
    stderr: Arc<PipeInodeInfo>,
}

impl Wasi {
    /// An invocation's context, granting the posture the Sandbox requested
    /// and presenting each of the invocation's Mounts as a preopen, from
    /// descriptor 3 in order.
    pub(crate) fn for_invocation(
        config: &Config,
        stdin: Vec<u8>,
        grants: &[Grant],
        open_connections: &Arc<OpenConnections>,
    ) -> Result<Self, Trap> {
        let mut wasi = Self::new(
            stdin,
            config.stdout_limit,
            config.stderr_limit,
            config.open_file_limit,
            prepare_creds(config),
            config.profile,
            grants.iter().filter_map(to_mount).collect(),
        )?;
        if let Some(network) = to_network(grants, open_connections, config.connection_limit)? {
            wasi.table.grant(network);
        }
        Ok(wasi)
    }

    /// A compile's context, at the strongest posture whatever the Sandbox
    /// requested: compiling the Snippets needs no ambient authority, so
    /// it is granted none.
    pub(crate) fn for_compile(config: &Config, stdin: Vec<u8>) -> Result<Self, Trap> {
        Self::new(
            stdin,
            config.stdout_limit,
            config.stderr_limit,
            config.open_file_limit,
            prepare_creds(config),
            Profile::Hermetic,
            Vec::new(),
        )
    }

    /// The ABI version probe's context. Instantiating it runs the
    /// artifact's start section, yet reading a version needs no input, no
    /// ambient authority, and no output: the probe takes an empty stdin, the
    /// strongest posture, and pipes sized to keep nothing.
    pub(crate) fn for_probe() -> Result<Self, Trap> {
        Self::new(
            Vec::new(),
            Some(0),
            Some(0),
            None,
            prepare_creds_within(&[], None),
            Profile::Hermetic,
            Vec::new(),
        )
    }

    /// An uncapped output channel has no ceiling on the host: a guest
    /// rewriting one buffer grows the pipe, not its own memory.
    fn new(
        stdin: Vec<u8>,
        stdout_limit: Option<usize>,
        stderr_limit: Option<usize>,
        open_file_limit: Option<usize>,
        opener: Arc<Cred>,
        profile: Profile,
        mounts: Vec<Mount>,
    ) -> Result<Self, Trap> {
        let stdout = PipeInodeInfo::new(endpoint::pipe_capacity(stdout_limit));
        let stderr = PipeInodeInfo::new(endpoint::pipe_capacity(stderr_limit));

        // stdio lives in kobako's descriptor table, which answers every
        // call that reads a descriptor (`crate::preview1`); the builder
        // keeps only what the remaining imports read. preview2's resource
        // table numbers its own and counts against the same open files.
        let open_files = OpenFiles::new(open_file_limit);
        let table = preview2::Table::new(open_files.clone());
        let descriptors = Descriptors::new(
            create_pipe_files(&PipeInodeInfo::from_bytes(stdin), &opener).0,
            create_pipe_files(&stdout, &opener).1,
            create_pipe_files(&stderr, &opener).1,
            mounts,
            open_files,
            opener,
            profile,
        )
        .map_err(|err| Trap::Other(format!("a Mount's root did not open: {err:?}")))?;
        // The Cred the Mounts' Policies restrict, which preview1's table and
        // preview2's calls share, as every fd a task holds shares its cred.
        let cred = descriptors.cred().clone();

        let mut builder = WasiCtxBuilder::new();
        // The profile decides the ambient authority granted: the
        // hermetic rung denies the preview1 time and entropy imports (see
        // `ambient`), the permissive rung wires the live clocks that the
        // guest's waits read too, and leaves the live entropy.
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
                let wall = ambient::LiveWallClock;
                builder.monotonic_clock(monotonic);
                builder.wall_clock(wall);
                Clocks::Live { monotonic, wall }
            }
        };

        Ok(Self {
            ctx: builder.build_p1(),
            cred,
            profile,
            descriptors,
            table,
            clocks,
            stdout,
            stderr,
        })
    }

    pub(crate) fn ctx_mut(&mut self) -> &mut WasiP1Ctx {
        &mut self.ctx
    }

    pub(crate) fn cred(&self) -> &Arc<Cred> {
        &self.cred
    }

    pub(crate) fn profile(&self) -> Profile {
        self.profile
    }

    pub(crate) fn descriptors(&self) -> &Descriptors {
        &self.descriptors
    }

    pub(crate) fn table(&self) -> &preview2::Table {
        &self.table
    }

    pub(crate) fn table_mut(&mut self) -> &mut preview2::Table {
        &mut self.table
    }

    pub(crate) fn descriptors_mut(&mut self) -> &mut Descriptors {
        &mut self.descriptors
    }

    pub(crate) fn clocks(&self) -> Clocks {
        self.clocks
    }

    pub(crate) fn stdout_bytes(&self) -> Vec<u8> {
        self.stdout.contents()
    }

    pub(crate) fn stderr_bytes(&self) -> Vec<u8> {
        self.stderr.contents()
    }
}

/// The open connections a fresh Sandbox counts, for a test building an
/// invocation's context as a Driver does.
#[cfg(test)]
pub(crate) fn sandbox_connections() -> Arc<OpenConnections> {
    OpenConnections::new(Config::default().open_connection_limit)
}

/// The network an invocation's Grants make, one namespace for every entry,
/// dialed through the host's own TCP, each connection counted against the
/// Sandbox's `open_connections` while open and the run's `connection_limit`
/// in all. No network Grant makes none.
fn to_network(
    grants: &[Grant],
    open_connections: &Arc<OpenConnections>,
    connection_limit: Option<usize>,
) -> Result<Option<Namespace>, Trap> {
    let entries = grants
        .iter()
        .filter_map(|grant| match grant {
            Grant::Network(network) => Some(network),
            _ => None,
        })
        .collect::<Vec<_>>();
    if entries.is_empty() {
        return Ok(None);
    }
    Namespace::new(
        entries,
        Arc::new(HostInet),
        Arc::new(HyperOutbound::new()),
        open_connections.clone(),
        connection_limit,
    )
    .map(Some)
    .map_err(|err| Trap::Other(format!("a network entry did not compile: {err}")))
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
    use crate::cache::load_engine;
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
        let engine = load_engine().expect("shared engine must be constructible");
        let config = Config::for_profile(profile);
        let wasi = Wasi::for_invocation(&config, Vec::new(), &[], &sandbox_connections())
            .expect("WASI context must build");
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
