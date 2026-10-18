//! Witness what a guest sees through a Mount: the preopen at the Mount's
//! path, the files a Filesystem holds, and the refusals the runtime
//! answers before the Filesystem is asked.

use std::sync::Arc;

use kobako_runtime::filesystem::{Filesystem, Memory, OpenOptions, Temporary};
use kobako_runtime::grant::Grant;
use kobako_runtime::mount::{Access, Mount, Policy};
use kobako_runtime::profile::Profile;
use wasmtime::{Instance, Module, Store};

use crate::cache::shared_engine;
use crate::config::Config;
use crate::imports;
use crate::invocation::Invocation;
use crate::trap::NO_TIMEOUT_EPOCH_DELTA;
use crate::wasi::Wasi;

const ERRNO_ACCES: i32 = 2;
const ERRNO_NOENT: i32 = 44;
const ERRNO_ROFS: i32 = 69;
const ERRNO_NOTCAPABLE: i32 = 76;

/// Memory holds an `iovec` at 64 over 512..576, a `ciovec` at 80 over
/// "hello" at 600, and the paths `file.txt` at 700 and `../file.txt` at
/// 720. An opened descriptor lands at 128 and a count at 132.
const PROBE_WAT: &str = r#"
(module
  (import "wasi_snapshot_preview1" "fd_prestat_get" (func $prestat (param i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_prestat_dir_name" (func $prestat_name (param i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "path_open" (func $open (param i32 i32 i32 i32 i32 i64 i64 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_read" (func $read (param i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_write" (func $write (param i32 i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 64) "\00\02\00\00\40\00\00\00")
  (data (i32.const 80) "\58\02\00\00\05\00\00\00")
  (data (i32.const 600) "hello")
  (data (i32.const 700) "file.txt")
  (data (i32.const 720) "../file.txt")

  (func (export "preopen_name") (result i32)
    (local $errno i32)
    (local.set $errno (call $prestat (i32.const 3) (i32.const 256)))
    (if (local.get $errno) (then (return (local.get $errno))))
    (call $prestat_name (i32.const 3) (i32.const 512) (i32.load (i32.const 260))))

  ;; Open the path read-only and read it into 512.
  (func (export "read_file") (param $path i32) (param $len i32) (result i32)
    (local $errno i32)
    (local.set $errno (call $open (i32.const 3) (i32.const 0) (local.get $path) (local.get $len)
      (i32.const 0) (i64.const 2) (i64.const 0) (i32.const 0) (i32.const 128)))
    (if (local.get $errno) (then (return (local.get $errno))))
    (call $read (i32.load (i32.const 128)) (i32.const 64) (i32.const 1) (i32.const 132)))

  ;; Open the path with read rights only, asking to truncate it.
  (func (export "truncate_file") (param $path i32) (param $len i32) (result i32)
    (call $open (i32.const 3) (i32.const 0) (local.get $path) (local.get $len)
      (i32.const 8) (i64.const 2) (i64.const 0) (i32.const 0) (i32.const 128)))

  ;; Create or truncate the path and write "hello" into it.
  (func (export "write_file") (param $path i32) (param $len i32) (result i32)
    (local $errno i32)
    (local.set $errno (call $open (i32.const 3) (i32.const 0) (local.get $path) (local.get $len)
      (i32.const 9) (i64.const 64) (i64.const 0) (i32.const 0) (i32.const 128)))
    (if (local.get $errno) (then (return (local.get $errno))))
    (call $write (i32.load (i32.const 128)) (i32.const 80) (i32.const 1) (i32.const 132))))
"#;

const FILE: (i32, i32) = (700, 8);
const ABOVE_ROOT: (i32, i32) = (720, 11);

fn grant(
    filesystem: Arc<dyn Filesystem>,
    access: Access,
    policy: Option<Arc<dyn Policy>>,
) -> Grant {
    let mount = Mount::new("/data", filesystem, access);
    Grant::Mount(match policy {
        Some(policy) => mount.with_policy(policy),
        None => mount,
    })
}

/// One invocation's guest, handed `grants`.
struct Guest {
    store: Store<Invocation>,
    instance: Instance,
}

impl Guest {
    fn new(grants: &[Grant]) -> Self {
        let engine = shared_engine().expect("shared engine must be constructible");
        let config = Config {
            timeout: None,
            memory_limit: None,
            stdout_limit: None,
            stderr_limit: None,
            open_file_limit: None,
            profile: Profile::Hermetic,
        };
        let wasi = Wasi::for_invocation(&config, &[], grants).expect("WASI context must build");
        let mut store = Store::new(engine, Invocation::new(None, wasi));
        store.set_epoch_deadline(NO_TIMEOUT_EPOCH_DELTA);
        let module = Module::new(engine, PROBE_WAT).expect("probe module must compile");
        let instance = imports::linker()
            .expect("host imports must link")
            .instantiate(&mut store, &module)
            .expect("probe module must instantiate");
        Self { store, instance }
    }

    fn call(&mut self, name: &str, path: (i32, i32)) -> i32 {
        self.instance
            .get_typed_func::<(i32, i32), i32>(&mut self.store, name)
            .expect("probe export must resolve")
            .call(&mut self.store, path)
            .expect("probe call must not trap")
    }

    fn preopen_name(&mut self) -> i32 {
        self.instance
            .get_typed_func::<(), i32>(&mut self.store, "preopen_name")
            .expect("probe export must resolve")
            .call(&mut self.store, ())
            .expect("probe call must not trap")
    }

    /// The bytes the last read placed at 512.
    fn read_back(&mut self) -> Vec<u8> {
        let memory = self
            .instance
            .get_memory(&mut self.store, "memory")
            .expect("probe exports its memory");
        let data = memory.data(&self.store);
        let read = u32::from_le_bytes(data[132..136].try_into().unwrap()) as usize;
        data[512..512 + read].to_vec()
    }
}

fn contents(filesystem: &dyn Filesystem, path: &str) -> Option<Vec<u8>> {
    let mut file = filesystem
        .open(
            path,
            &OpenOptions {
                read: true,
                ..OpenOptions::default()
            },
        )
        .ok()?;
    let mut buf = vec![0; 64];
    let read = file.read_at(&mut buf, 0).ok()?;
    buf.truncate(read);
    Some(buf)
}

struct Refuses(&'static str);

impl Policy for Refuses {
    fn allows(&self, path: &str, _access: Access) -> bool {
        path != self.0
    }
}

struct AllowsAll;

impl Policy for AllowsAll {
    fn allows(&self, _path: &str, _access: Access) -> bool {
        true
    }
}

// @behavior RT-075
#[test]
fn a_mount_appears_as_a_directory_at_its_path() {
    let mut guest = Guest::new(&[grant(Arc::new(Memory::new()), Access::Read, None)]);
    assert_eq!(
        guest.preopen_name(),
        0,
        "the first preopen must answer its name"
    );
    let memory = guest
        .instance
        .get_memory(&mut guest.store, "memory")
        .expect("probe exports its memory");
    assert_eq!(
        &memory.data(&guest.store)[512..517],
        b"/data",
        "a Mount at /data through Runtime::invoke must preopen /data as descriptor 3"
    );
}

// @behavior RT-076
#[test]
fn a_guest_reads_a_file_beneath_a_mount() {
    let memory: Memory = [("file.txt", "granted")].into_iter().collect();
    let mut guest = Guest::new(&[grant(Arc::new(memory), Access::Read, None)]);
    assert_eq!(guest.call("read_file", FILE), 0, "the read must succeed");
    assert_eq!(
        guest.read_back(),
        b"granted",
        "a file read beneath a Mount through preview1 must read the Memory's content"
    );
}

// @behavior RT-077
#[test]
fn a_guest_writes_a_file_beneath_a_read_write_mount() {
    let memory = Arc::new(Memory::new());
    let mut guest = Guest::new(&[grant(memory.clone(), Access::ReadWrite, None)]);
    assert_eq!(guest.call("write_file", FILE), 0, "the write must succeed");
    assert_eq!(
        contents(memory.as_ref(), "file.txt").as_deref(),
        Some(&b"hello"[..]),
        "a file written beneath a read-write Mount through preview1 must land in the Memory"
    );
}

// @behavior RT-078
#[test]
fn a_change_beneath_a_read_only_mount_is_a_read_only_filesystem() {
    let mut guest = Guest::new(&[grant(Arc::new(Memory::new()), Access::Read, None)]);
    assert_eq!(
        guest.call("write_file", FILE),
        ERRNO_ROFS,
        "a write opened beneath a read-only Mount through preview1 must answer EROFS"
    );
}

// An open asking only to read can still truncate. wasmtime-wasi once let
// such an open past a read-only file permission (CVE-2026-47261), so the
// truncation alone must count as a change.
// @behavior RT-078
#[test]
fn a_truncating_open_beneath_a_read_only_mount_is_refused() {
    let memory: Arc<Memory> = Arc::new([("file.txt", "granted")].into_iter().collect());
    let mut guest = Guest::new(&[grant(memory.clone(), Access::Read, None)]);
    assert_eq!(
        (
            guest.call("truncate_file", FILE),
            contents(memory.as_ref(), "file.txt")
        ),
        (ERRNO_ROFS, Some(b"granted".to_vec())),
        "a read-rights open with O_TRUNC beneath a read-only Mount through preview1 must answer \
         EROFS and leave the file whole"
    );
}

// @behavior RT-079
#[test]
fn a_path_above_a_mounts_root_is_refused() {
    let memory: Memory = [("file.txt", "granted")].into_iter().collect();
    let mut guest = Guest::new(&[grant(Arc::new(memory), Access::Read, None)]);
    assert_eq!(
        guest.call("read_file", ABOVE_ROOT),
        ERRNO_NOTCAPABLE,
        "a path climbing above a Mount's root through preview1 must answer ENOTCAPABLE"
    );
}

// @behavior RT-080
#[test]
fn a_policy_refusing_an_open_answers_access_denied() {
    let memory: Memory = [("file.txt", "granted")].into_iter().collect();
    let policy: Arc<dyn Policy> = Arc::new(Refuses("/data/file.txt"));
    let mut guest = Guest::new(&[grant(Arc::new(memory), Access::Read, Some(policy))]);
    assert_eq!(
        guest.call("read_file", FILE),
        ERRNO_ACCES,
        "an open a Mount's Policy refuses through preview1 must answer EACCES"
    );
}

// @behavior RT-081
#[test]
fn a_policy_decides_in_the_place_of_a_mounts_access() {
    let memory = Arc::new(Memory::new());
    let policy: Arc<dyn Policy> = Arc::new(AllowsAll);
    let mut guest = Guest::new(&[grant(memory.clone(), Access::Read, Some(policy))]);
    assert_eq!(guest.call("write_file", FILE), 0, "the write must succeed");
    assert_eq!(
        contents(memory.as_ref(), "file.txt").as_deref(),
        Some(&b"hello"[..]),
        "a write a Policy allows beneath a read-only Mount through preview1 must land"
    );
}

// @behavior RT-082
#[test]
fn each_invocation_is_handed_the_filesystem_it_starts_from() {
    let grants = [grant(Arc::new(Temporary::new()), Access::ReadWrite, None)];
    assert_eq!(
        Guest::new(&grants).call("write_file", FILE),
        0,
        "the write must succeed"
    );
    assert_eq!(
        Guest::new(&grants).call("read_file", FILE),
        ERRNO_NOENT,
        "a file an earlier invocation wrote to a Temporary must not be found by the next one"
    );
}
