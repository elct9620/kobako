//! The differential probe: every call kobako answers, made on descriptors
//! 0 to 3 against kobako's linker and against plain wasmtime-wasi preview1,
//! must leave the same errno, the same guest memory, and the same captures.
//!
//! It guards each wasmtime-wasi upgrade: a change in what preview1 answers
//! shows up here rather than in a guest. The deliberate differences — a
//! clock wait under the hermetic rung — are witnessed in `poll`.

mod mount;

use std::sync::OnceLock;

use wasmtime::{Linker, Module, Store};
use wasmtime_wasi::p1::{self, WasiP1Ctx};
use wasmtime_wasi::p2::pipe::{MemoryInputPipe, MemoryOutputPipe};
use wasmtime_wasi::WasiCtxBuilder;

use crate::cache::shared_engine;
use crate::config::Config;
use crate::imports;
use crate::invocation::Invocation;
use crate::trap::NO_TIMEOUT_EPOCH_DELTA;
use crate::wasi::Wasi;
use kobako_runtime::profile::Profile;

/// The Preamble bytes stdin carries after its length prefix.
const PREAMBLE: &[u8] = b"hello";

/// Every export takes a descriptor and answers the call's errno. Memory
/// holds an `iovec` at 64 over 512..528, an empty one at 72, a `ciovec` at 80 over "hello" at
/// 600, and the paths "a" and "b" at 700 and 702; results land at 128 and
/// 256, subscriptions are built at 800 and events written at 896.
const PROBE_WAT: &str = r#"
(module
  (import "wasi_snapshot_preview1" "fd_advise" (func $fd_advise (param i32 i64 i64 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_allocate" (func $fd_allocate (param i32 i64 i64) (result i32)))
  (import "wasi_snapshot_preview1" "fd_close" (func $fd_close (param i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_datasync" (func $fd_datasync (param i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_fdstat_get" (func $fd_fdstat_get (param i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_fdstat_set_flags" (func $fd_fdstat_set_flags (param i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_fdstat_set_rights" (func $fd_fdstat_set_rights (param i32 i64 i64) (result i32)))
  (import "wasi_snapshot_preview1" "fd_filestat_get" (func $fd_filestat_get (param i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_filestat_set_size" (func $fd_filestat_set_size (param i32 i64) (result i32)))
  (import "wasi_snapshot_preview1" "fd_filestat_set_times" (func $fd_filestat_set_times (param i32 i64 i64 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_pread" (func $fd_pread (param i32 i32 i32 i64 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_prestat_get" (func $fd_prestat_get (param i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_prestat_dir_name" (func $fd_prestat_dir_name (param i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_pwrite" (func $fd_pwrite (param i32 i32 i32 i64 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_read" (func $fd_read (param i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_readdir" (func $fd_readdir (param i32 i32 i32 i64 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_renumber" (func $fd_renumber (param i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_seek" (func $fd_seek (param i32 i64 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_sync" (func $fd_sync (param i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_tell" (func $fd_tell (param i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_write" (func $fd_write (param i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "path_create_directory" (func $path_create_directory (param i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "path_filestat_get" (func $path_filestat_get (param i32 i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "path_filestat_set_times" (func $path_filestat_set_times (param i32 i32 i32 i32 i64 i64 i32) (result i32)))
  (import "wasi_snapshot_preview1" "path_link" (func $path_link (param i32 i32 i32 i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "path_open" (func $path_open (param i32 i32 i32 i32 i32 i64 i64 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "path_readlink" (func $path_readlink (param i32 i32 i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "path_remove_directory" (func $path_remove_directory (param i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "path_rename" (func $path_rename (param i32 i32 i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "path_symlink" (func $path_symlink (param i32 i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "path_unlink_file" (func $path_unlink_file (param i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "poll_oneoff" (func $poll_oneoff (param i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "sock_accept" (func $sock_accept (param i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "sock_recv" (func $sock_recv (param i32 i32 i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "sock_send" (func $sock_send (param i32 i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "sock_shutdown" (func $sock_shutdown (param i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 64) "\00\02\00\00\10\00\00\00")
  (data (i32.const 72) "\00\02\00\00\00\00\00\00")
  (data (i32.const 80) "\58\02\00\00\05\00\00\00")
  (data (i32.const 600) "hello")
  (data (i32.const 700) "a\00b")

  ;; One subscription at 800: userdata 7, the tag, and the descriptor.
  (func $subscribe (param $tag i32) (param $fd i32)
    (i64.store (i32.const 800) (i64.const 7))
    (i32.store8 (i32.const 808) (local.get $tag))
    (i32.store (i32.const 816) (local.get $fd)))

  (func (export "fd_advise") (param i32) (result i32) (call $fd_advise (local.get 0) (i64.const 0) (i64.const 0) (i32.const 0)))
  (func (export "fd_allocate") (param i32) (result i32) (call $fd_allocate (local.get 0) (i64.const 0) (i64.const 0)))
  (func (export "fd_close") (param i32) (result i32) (call $fd_close (local.get 0)))
  (func (export "fd_datasync") (param i32) (result i32) (call $fd_datasync (local.get 0)))
  (func (export "fd_fdstat_get") (param i32) (result i32) (call $fd_fdstat_get (local.get 0) (i32.const 256)))
  (func (export "fd_fdstat_set_flags") (param i32) (result i32) (call $fd_fdstat_set_flags (local.get 0) (i32.const 0)))
  (func (export "fd_fdstat_set_rights") (param i32) (result i32) (call $fd_fdstat_set_rights (local.get 0) (i64.const 0) (i64.const 0)))
  (func (export "fd_filestat_get") (param i32) (result i32) (call $fd_filestat_get (local.get 0) (i32.const 256)))
  (func (export "fd_filestat_set_size") (param i32) (result i32) (call $fd_filestat_set_size (local.get 0) (i64.const 0)))
  (func (export "fd_filestat_set_times") (param i32) (result i32) (call $fd_filestat_set_times (local.get 0) (i64.const 0) (i64.const 0) (i32.const 0)))
  (func (export "fd_pread") (param i32) (result i32) (call $fd_pread (local.get 0) (i32.const 64) (i32.const 1) (i64.const 0) (i32.const 128)))
  (func (export "fd_prestat_get") (param i32) (result i32) (call $fd_prestat_get (local.get 0) (i32.const 256)))
  (func (export "fd_prestat_dir_name") (param i32) (result i32) (call $fd_prestat_dir_name (local.get 0) (i32.const 512) (i32.const 16)))
  (func (export "fd_pwrite") (param i32) (result i32) (call $fd_pwrite (local.get 0) (i32.const 80) (i32.const 1) (i64.const 0) (i32.const 128)))
  (func (export "fd_read") (param i32) (result i32) (call $fd_read (local.get 0) (i32.const 64) (i32.const 1) (i32.const 128)))
  (func (export "fd_readdir") (param i32) (result i32) (call $fd_readdir (local.get 0) (i32.const 512) (i32.const 64) (i64.const 0) (i32.const 128)))
  (func (export "fd_renumber") (param i32) (result i32) (call $fd_renumber (local.get 0) (local.get 0)))
  (func (export "fd_seek") (param i32) (result i32) (call $fd_seek (local.get 0) (i64.const 0) (i32.const 0) (i32.const 128)))
  (func (export "fd_sync") (param i32) (result i32) (call $fd_sync (local.get 0)))
  (func (export "fd_tell") (param i32) (result i32) (call $fd_tell (local.get 0) (i32.const 128)))
  (func (export "fd_write") (param i32) (result i32) (call $fd_write (local.get 0) (i32.const 80) (i32.const 1) (i32.const 128)))
  (func (export "path_create_directory") (param i32) (result i32) (call $path_create_directory (local.get 0) (i32.const 700) (i32.const 1)))
  (func (export "path_filestat_get") (param i32) (result i32) (call $path_filestat_get (local.get 0) (i32.const 0) (i32.const 700) (i32.const 1) (i32.const 256)))
  (func (export "path_filestat_set_times") (param i32) (result i32) (call $path_filestat_set_times (local.get 0) (i32.const 0) (i32.const 700) (i32.const 1) (i64.const 0) (i64.const 0) (i32.const 0)))
  (func (export "path_link") (param i32) (result i32) (call $path_link (local.get 0) (i32.const 0) (i32.const 700) (i32.const 1) (local.get 0) (i32.const 702) (i32.const 1)))
  (func (export "path_open") (param i32) (result i32) (call $path_open (local.get 0) (i32.const 0) (i32.const 700) (i32.const 1) (i32.const 0) (i64.const 0) (i64.const 0) (i32.const 0) (i32.const 128)))
  (func (export "path_readlink") (param i32) (result i32) (call $path_readlink (local.get 0) (i32.const 700) (i32.const 1) (i32.const 512) (i32.const 16) (i32.const 128)))
  (func (export "path_remove_directory") (param i32) (result i32) (call $path_remove_directory (local.get 0) (i32.const 700) (i32.const 1)))
  (func (export "path_rename") (param i32) (result i32) (call $path_rename (local.get 0) (i32.const 700) (i32.const 1) (local.get 0) (i32.const 702) (i32.const 1)))
  (func (export "path_symlink") (param i32) (result i32) (call $path_symlink (i32.const 700) (i32.const 1) (local.get 0) (i32.const 702) (i32.const 1)))
  (func (export "path_unlink_file") (param i32) (result i32) (call $path_unlink_file (local.get 0) (i32.const 700) (i32.const 1)))
  (func (export "poll_oneoff_read") (param i32) (result i32)
    (call $subscribe (i32.const 1) (local.get 0))
    (call $poll_oneoff (i32.const 800) (i32.const 896) (i32.const 1) (i32.const 128)))
  (func (export "poll_oneoff_write") (param i32) (result i32)
    (call $subscribe (i32.const 2) (local.get 0))
    (call $poll_oneoff (i32.const 800) (i32.const 896) (i32.const 1) (i32.const 128)))
  (func (export "poll_oneoff_none") (param i32) (result i32)
    (call $poll_oneoff (i32.const 800) (i32.const 896) (i32.const 0) (i32.const 128)))
  (func (export "sock_accept") (param i32) (result i32) (call $sock_accept (local.get 0) (i32.const 0) (i32.const 128)))
  (func (export "sock_recv") (param i32) (result i32) (call $sock_recv (local.get 0) (i32.const 64) (i32.const 1) (i32.const 0) (i32.const 128) (i32.const 132)))
  (func (export "sock_send") (param i32) (result i32) (call $sock_send (local.get 0) (i32.const 80) (i32.const 1) (i32.const 0) (i32.const 128)))
  (func (export "sock_shutdown") (param i32) (result i32) (call $sock_shutdown (local.get 0) (i32.const 0)))

  (func (export "fd_read_empty") (param i32) (result i32) (call $fd_read (local.get 0) (i32.const 72) (i32.const 1) (i32.const 128)))
  (func (export "fd_write_empty") (param i32) (result i32) (call $fd_write (local.get 0) (i32.const 72) (i32.const 1) (i32.const 128)))
  (func (export "close_twice") (param i32) (result i32)
    (drop (call $fd_close (local.get 0)))
    (call $fd_close (local.get 0)))
  (func (export "renumber_onto_stdin") (param i32) (result i32)
    (drop (call $fd_renumber (local.get 0) (i32.const 0)))
    (call $fd_fdstat_get (i32.const 0) (i32.const 256)))
  (func (export "read_twice") (param i32) (result i32)
    (drop (call $fd_read (local.get 0) (i32.const 64) (i32.const 1) (i32.const 128)))
    (call $fd_read (local.get 0) (i32.const 64) (i32.const 1) (i32.const 132)))
  (func (export "write_twice") (param i32) (result i32)
    (drop (call $fd_write (local.get 0) (i32.const 80) (i32.const 1) (i32.const 128)))
    (call $fd_write (local.get 0) (i32.const 80) (i32.const 1) (i32.const 132))))
"#;

const CALLS: &[&str] = &[
    "fd_advise",
    "fd_allocate",
    "fd_close",
    "fd_datasync",
    "fd_fdstat_get",
    "fd_fdstat_set_flags",
    "fd_fdstat_set_rights",
    "fd_filestat_get",
    "fd_filestat_set_size",
    "fd_filestat_set_times",
    "fd_pread",
    "fd_prestat_get",
    "fd_prestat_dir_name",
    "fd_pwrite",
    "fd_read",
    "fd_readdir",
    "fd_renumber",
    "fd_seek",
    "fd_sync",
    "fd_tell",
    "fd_write",
    "path_create_directory",
    "path_filestat_get",
    "path_filestat_set_times",
    "path_link",
    "path_open",
    "path_readlink",
    "path_remove_directory",
    "path_rename",
    "path_symlink",
    "path_unlink_file",
    "poll_oneoff_read",
    "poll_oneoff_write",
    "poll_oneoff_none",
    "sock_accept",
    "sock_recv",
    "sock_send",
    "sock_shutdown",
    "fd_read_empty",
    "fd_write_empty",
    "close_twice",
    "renumber_onto_stdin",
    "read_twice",
    "write_twice",
];

/// What one call leaves behind; `None` for a call that trapped.
#[derive(Debug, PartialEq)]
struct Observation {
    errno: Option<i32>,
    memory: Vec<u8>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

/// A capped Channel exercises the pipe filling mid-write.
const CHANNEL_LIMITS: [Option<usize>; 2] = [None, Some(3)];

fn stdin_bytes() -> Vec<u8> {
    let mut bytes = (PREAMBLE.len() as u32).to_be_bytes().to_vec();
    bytes.extend_from_slice(PREAMBLE);
    bytes
}

fn observe<T>(
    mut store: Store<T>,
    linker: &Linker<T>,
    call: &str,
    fd: i32,
) -> (Observation, Store<T>) {
    static MODULE: OnceLock<Module> = OnceLock::new();
    let module = MODULE
        .get_or_init(|| Module::new(store.engine(), PROBE_WAT).expect("probe module must compile"));
    let instance = linker
        .instantiate(&mut store, module)
        .expect("probe module must instantiate");
    let errno = instance
        .get_typed_func::<i32, i32>(&mut store, call)
        .expect("probe export must resolve")
        .call(&mut store, fd)
        .ok();
    let memory = instance
        .get_memory(&mut store, "memory")
        .expect("probe exports its memory")
        .data(&store)[..1024]
        .to_vec();
    let observation = Observation {
        errno,
        memory,
        stdout: Vec::new(),
        stderr: Vec::new(),
    };
    (observation, store)
}

fn through_kobako(call: &str, fd: i32, limit: Option<usize>) -> Observation {
    let engine = shared_engine().expect("shared engine must be constructible");
    let config = Config {
        timeout: None,
        setup_timeout: None,
        memory_limit: None,
        stdout_limit: limit,
        stderr_limit: limit,
        open_file_limit: None,
        profile: Profile::Hermetic,
    };
    let wasi = Wasi::for_invocation(&config, PREAMBLE, &[]).expect("WASI context must build");
    let mut store = Store::new(engine, Invocation::new(None, wasi));
    store.set_epoch_deadline(NO_TIMEOUT_EPOCH_DELTA);
    let linker = imports::linker().expect("host imports must link");
    let (mut observation, store) = observe(store, &linker, call, fd);
    observation.stdout = store.data().wasi().stdout_bytes();
    observation.stderr = store.data().wasi().stderr_bytes();
    observation
}

/// Plain wasmtime-wasi preview1 over the same stdin and pipes.
struct Oracle {
    ctx: WasiP1Ctx,
    stdout: MemoryOutputPipe,
    stderr: MemoryOutputPipe,
}

fn through_wasmtime_wasi(call: &str, fd: i32, limit: Option<usize>) -> Observation {
    let engine = shared_engine().expect("shared engine must be constructible");
    let stdout = MemoryOutputPipe::new(crate::capture::pipe_capacity(limit));
    let stderr = MemoryOutputPipe::new(crate::capture::pipe_capacity(limit));
    let ctx = WasiCtxBuilder::new()
        .stdin(MemoryInputPipe::new(stdin_bytes()))
        .stdout(stdout.clone())
        .stderr(stderr.clone())
        .build_p1();
    let mut store = Store::new(
        engine,
        Oracle {
            ctx,
            stdout,
            stderr,
        },
    );
    store.set_epoch_deadline(NO_TIMEOUT_EPOCH_DELTA);
    let mut linker = Linker::new(engine);
    p1::add_to_linker_sync(&mut linker, |oracle: &mut Oracle| &mut oracle.ctx)
        .expect("preview1 must link");
    let (mut observation, store) = observe(store, &linker, call, fd);
    observation.stdout = store.data().stdout.contents().to_vec();
    observation.stderr = store.data().stderr.contents().to_vec();
    observation
}

#[test]
fn every_descriptor_call_answers_as_wasmtime_wasi_preview1() {
    for limit in CHANNEL_LIMITS {
        for call in CALLS {
            for fd in 0..=3 {
                assert_eq!(
                    through_kobako(call, fd, limit),
                    through_wasmtime_wasi(call, fd, limit),
                    "{call} on descriptor {fd} with Channels capped at {limit:?} through \
                     kobako's preview1 imports must answer as wasmtime-wasi's preview1 does"
                );
            }
        }
    }
}
