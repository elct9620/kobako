//! Witness what a guest sees through a Mount: the preopen at the Mount's
//! path, the files a Filesystem holds, and the refusals the runtime
//! answers before the Filesystem is asked.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use kobako_runtime::filesystem::{
    DirEntry, File, Filesystem, FilesystemError, Memory, Metadata, OpenOptions, Rename, Temporary,
};
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
const ERRNO_BUSY: i32 = 10;
const ERRNO_INVAL: i32 = 28;
const ERRNO_ISDIR: i32 = 31;
const ERRNO_NOENT: i32 = 44;
const ERRNO_NOTDIR: i32 = 54;
const ERRNO_ROFS: i32 = 69;
const ERRNO_XDEV: i32 = 75;
const ERRNO_NOTCAPABLE: i32 = 76;

/// Memory holds an `iovec` at 64 over 512..576, a `ciovec` at 80 over
/// "hello" at 600, the paths `file.txt` at 700 and `../file.txt` at 720,
/// and from 740 the paths the resolution rules turn on. An opened
/// descriptor lands at 128 and a count at 132.
const PROBE_WAT: &str = r#"
(module
  (import "wasi_snapshot_preview1" "fd_prestat_get" (func $prestat (param i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_prestat_dir_name" (func $prestat_name (param i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "path_open" (func $open (param i32 i32 i32 i32 i32 i64 i64 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_read" (func $read (param i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_write" (func $write (param i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "path_remove_directory" (func $rmdir (param i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "path_rename" (func $rename (param i32 i32 i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "path_unlink_file" (func $unlink (param i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 740) ".")
  (data (i32.const 760) "missing/../file.txt")
  (data (i32.const 780) "file.txt/..")
  (data (i32.const 800) "file.txt/")
  (data (i32.const 820) "new.txt/")
  (data (i32.const 840) "new.txt")
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
    (call $write (i32.load (i32.const 128)) (i32.const 80) (i32.const 1) (i32.const 132)))

  ;; Open the path read-only with the given oflags and fdflags.
  (func (export "open_flagged") (param $path i32) (param $len i32) (param $oflags i32) (param $fdflags i32) (result i32)
    (call $open (i32.const 3) (i32.const 0) (local.get $path) (local.get $len)
      (local.get $oflags) (i64.const 2) (i64.const 0) (local.get $fdflags) (i32.const 128)))

  ;; Change the root `.` at 740.
  (func (export "remove_root") (result i32)
    (call $rmdir (i32.const 3) (i32.const 740) (i32.const 1)))
  (func (export "rename_root") (result i32)
    (call $rename (i32.const 3) (i32.const 740) (i32.const 1) (i32.const 3) (i32.const 700) (i32.const 8)))
  (func (export "rename_onto_root") (result i32)
    (call $rename (i32.const 3) (i32.const 700) (i32.const 8) (i32.const 3) (i32.const 740) (i32.const 1)))
  ;; Open the path to write and append, and write "hello" into it.
  (func (export "append_file") (param $path i32) (param $len i32) (result i32)
    (local $errno i32)
    (local.set $errno (call $open (i32.const 3) (i32.const 0) (local.get $path) (local.get $len)
      (i32.const 0) (i64.const 64) (i64.const 0) (i32.const 1) (i32.const 128)))
    (if (local.get $errno) (then (return (local.get $errno))))
    (call $write (i32.load (i32.const 128)) (i32.const 80) (i32.const 1) (i32.const 132)))

  ;; Rename `file.txt` to `new.txt`.
  (func (export "rename_to_new") (result i32)
    (call $rename (i32.const 3) (i32.const 700) (i32.const 8) (i32.const 3) (i32.const 840) (i32.const 7)))
  ;; Rename `file.txt` onto `file.txt/`, its own path as a directory.
  (func (export "rename_onto_itself_as_directory") (result i32)
    (call $rename (i32.const 3) (i32.const 700) (i32.const 8) (i32.const 3) (i32.const 800) (i32.const 9)))
  (func (export "unlink_root") (result i32)
    (call $unlink (i32.const 3) (i32.const 740) (i32.const 1))))
"#;

const FILE: (i32, i32) = (700, 8);
const ABOVE_ROOT: (i32, i32) = (720, 11);
const EMPTY: (i32, i32) = (700, 0);
const THROUGH_MISSING: (i32, i32) = (760, 19);
const OUT_OF_FILE: (i32, i32) = (780, 11);
const FILE_AS_DIRECTORY: (i32, i32) = (800, 9);
const NEW_AS_DIRECTORY: (i32, i32) = (820, 8);

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

    /// Open `path` read-only with `oflags` and `fdflags`.
    fn call_flagged(&mut self, path: (i32, i32), oflags: i32, fdflags: i32) -> i32 {
        self.instance
            .get_typed_func::<(i32, i32, i32, i32), i32>(&mut self.store, "open_flagged")
            .expect("probe export must resolve")
            .call(&mut self.store, (path.0, path.1, oflags, fdflags))
            .expect("probe call must not trap")
    }

    /// Call an export that takes no path.
    fn call_bare(&mut self, name: &str) -> i32 {
        self.instance
            .get_typed_func::<(), i32>(&mut self.store, name)
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
        guest.call_bare("preopen_name"),
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

/// A Memory holding `file.txt` that counts every change it is asked for,
/// so a test can tell the runtime refused before asking it.
struct Counted {
    memory: Memory,
    changes: AtomicUsize,
}

impl Counted {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            memory: [("file.txt", "granted")].into_iter().collect(),
            changes: AtomicUsize::new(0),
        })
    }

    fn change(&self) {
        self.changes.fetch_add(1, Ordering::Relaxed);
    }
}

impl Filesystem for Counted {
    fn open(&self, path: &str, options: &OpenOptions) -> Result<Box<dyn File>, FilesystemError> {
        self.memory.open(path, options)
    }

    fn metadata(&self, path: &str) -> Result<Metadata, FilesystemError> {
        self.memory.metadata(path)
    }

    fn read_dir(&self, path: &str) -> Result<Vec<DirEntry>, FilesystemError> {
        self.memory.read_dir(path)
    }

    fn create_dir(&self, path: &str) -> Result<(), FilesystemError> {
        self.change();
        self.memory.create_dir(path)
    }

    fn remove_dir(&self, path: &str) -> Result<(), FilesystemError> {
        self.change();
        self.memory.remove_dir(path)
    }

    fn remove_file(&self, path: &str) -> Result<(), FilesystemError> {
        self.change();
        self.memory.remove_file(path)
    }

    fn rename(&self, from: &str, to: &str, rename: Rename) -> Result<(), FilesystemError> {
        self.change();
        self.memory.rename(from, to, rename)
    }
}

/// A Memory holding `file.txt` that, as a rename reaches it, replaces the
/// file with a directory holding `secret`, as another invocation sharing
/// the Memory could between the runtime's checks and the move.
struct SwappedForADirectory(Memory);

impl SwappedForADirectory {
    fn new() -> Arc<Self> {
        Arc::new(Self([("file.txt", "granted")].into_iter().collect()))
    }
}

impl Filesystem for SwappedForADirectory {
    fn open(&self, path: &str, options: &OpenOptions) -> Result<Box<dyn File>, FilesystemError> {
        self.0.open(path, options)
    }

    fn metadata(&self, path: &str) -> Result<Metadata, FilesystemError> {
        self.0.metadata(path)
    }

    fn read_dir(&self, path: &str) -> Result<Vec<DirEntry>, FilesystemError> {
        self.0.read_dir(path)
    }

    fn create_dir(&self, path: &str) -> Result<(), FilesystemError> {
        self.0.create_dir(path)
    }

    fn remove_dir(&self, path: &str) -> Result<(), FilesystemError> {
        self.0.remove_dir(path)
    }

    fn remove_file(&self, path: &str) -> Result<(), FilesystemError> {
        self.0.remove_file(path)
    }

    fn rename(&self, from: &str, to: &str, rename: Rename) -> Result<(), FilesystemError> {
        self.0.remove_file(from)?;
        self.0.create_dir(from)?;
        drop(self.0.open(
            &format!("{from}/secret"),
            &OpenOptions {
                write: true,
                create: true,
                ..OpenOptions::default()
            },
        )?);
        self.0.rename(from, to, rename)
    }
}

// The Policy allows everything, so only the rename's own refusal of a
// directory, checked as the Filesystem moves it, keeps `secret` from
// taking a path the Policy never judged.
// @behavior RT-094
#[test]
fn a_directory_beneath_a_mount_with_a_policy_does_not_move() {
    let swapped = SwappedForADirectory::new();
    let policy: Arc<dyn Policy> = Arc::new(AllowsAll);
    let mut guest = Guest::new(&[grant(swapped.clone(), Access::ReadWrite, Some(policy))]);
    assert_eq!(
        (
            guest.call_bare("rename_to_new"),
            swapped.metadata("new.txt/secret")
        ),
        (ERRNO_XDEV, Err(FilesystemError::NotFound)),
        "path_rename of what is a directory by the time it moves, beneath a Mount with a \
         Policy, through preview1 must answer EXDEV and move nothing"
    );
}

/// A Memory whose files take another writer's `bbb` at their end as one
/// is written, once the length the write would land at is known and
/// before it lands, as another invocation sharing the file could.
struct AppendedMeanwhile(Memory);

struct AppendedMeanwhileFile {
    file: Box<dyn File>,
    other: std::sync::Mutex<Box<dyn File>>,
}

impl AppendedMeanwhileFile {
    fn other_appends(&self) -> Result<(), FilesystemError> {
        self.other
            .lock()
            .expect("the other writer's lock must not be poisoned")
            .append(b"bbb")
            .map(drop)
    }
}

impl File for AppendedMeanwhileFile {
    fn read_at(&mut self, buf: &mut [u8], offset: u64) -> Result<usize, FilesystemError> {
        self.file.read_at(buf, offset)
    }

    fn write_at(&mut self, buf: &[u8], offset: u64) -> Result<usize, FilesystemError> {
        self.file.write_at(buf, offset)
    }

    /// The length is read before the other writer appends, so a write
    /// placed by it lands over what the other wrote.
    fn metadata(&self) -> Result<Metadata, FilesystemError> {
        let metadata = self.file.metadata()?;
        self.other_appends()?;
        Ok(metadata)
    }

    fn set_len(&mut self, len: u64) -> Result<(), FilesystemError> {
        self.file.set_len(len)
    }

    fn append(&mut self, buf: &[u8]) -> Result<(u64, usize), FilesystemError> {
        self.other_appends()?;
        self.file.append(buf)
    }
}

impl Filesystem for AppendedMeanwhile {
    fn open(&self, path: &str, options: &OpenOptions) -> Result<Box<dyn File>, FilesystemError> {
        let writes = OpenOptions {
            write: true,
            ..OpenOptions::default()
        };
        Ok(Box::new(AppendedMeanwhileFile {
            file: self.0.open(path, options)?,
            other: std::sync::Mutex::new(self.0.open(path, &writes)?),
        }))
    }

    fn metadata(&self, path: &str) -> Result<Metadata, FilesystemError> {
        self.0.metadata(path)
    }

    fn read_dir(&self, path: &str) -> Result<Vec<DirEntry>, FilesystemError> {
        self.0.read_dir(path)
    }

    fn create_dir(&self, path: &str) -> Result<(), FilesystemError> {
        self.0.create_dir(path)
    }

    fn remove_dir(&self, path: &str) -> Result<(), FilesystemError> {
        self.0.remove_dir(path)
    }

    fn remove_file(&self, path: &str) -> Result<(), FilesystemError> {
        self.0.remove_file(path)
    }

    fn rename(&self, from: &str, to: &str, rename: Rename) -> Result<(), FilesystemError> {
        self.0.rename(from, to, rename)
    }
}

// The other writer's `bbb` lands as the guest's append is made, so only
// an append the file places itself keeps from writing over it.
// @behavior RT-095
#[test]
fn an_append_lands_after_another_writers_append() {
    let memory: Memory = [("file.txt", "granted")].into_iter().collect();
    let shared = Arc::new(AppendedMeanwhile(memory));
    let mut guest = Guest::new(&[grant(shared.clone(), Access::ReadWrite, None)]);
    assert_eq!(
        (
            guest.call("append_file", FILE),
            contents(&shared.0, "file.txt")
        ),
        (0, Some(b"grantedbbbhello".to_vec())),
        "fd_write to a file opened to append beneath a Mount through preview1 must land after \
         what another writer appended meanwhile"
    );
}

/// What each root-changing export answers, and how many changes the
/// Filesystem was asked for along the way.
fn change_root(exports: &[&str]) -> (Vec<i32>, usize) {
    let counted = Counted::new();
    let mut guest = Guest::new(&[grant(counted.clone(), Access::ReadWrite, None)]);
    let answers = exports.iter().map(|name| guest.call_bare(name)).collect();
    (answers, counted.changes.load(Ordering::Relaxed))
}

// @behavior RT-083
#[test]
fn removing_a_mounts_root_is_refused_as_invalid() {
    assert_eq!(
        change_root(&["remove_root"]),
        (vec![ERRNO_INVAL], 0),
        "path_remove_directory of `.` beneath a Mount through preview1 must answer EINVAL \
         without asking the Filesystem"
    );
}

// @behavior RT-084
#[test]
fn renaming_a_mounts_root_is_refused_as_busy() {
    assert_eq!(
        change_root(&["rename_root", "rename_onto_root"]),
        (vec![ERRNO_BUSY, ERRNO_BUSY], 0),
        "path_rename from or onto `.` beneath a Mount through preview1 must answer EBUSY \
         without asking the Filesystem"
    );
}

// @behavior RT-085
#[test]
fn unlinking_a_mounts_root_answers_that_it_is_a_directory() {
    assert_eq!(
        change_root(&["unlink_root"]),
        (vec![ERRNO_ISDIR], 0),
        "path_unlink_file of `.` beneath a Mount through preview1 must answer EISDIR \
         without asking the Filesystem"
    );
}

/// What reading `path` answers beneath a read-only Mount holding
/// `file.txt`.
fn read_beneath(path: (i32, i32)) -> i32 {
    let memory: Memory = [("file.txt", "granted")].into_iter().collect();
    Guest::new(&[grant(Arc::new(memory), Access::Read, None)]).call("read_file", path)
}

// @behavior RT-086
#[test]
fn an_empty_path_names_nothing_beneath_a_mount() {
    assert_eq!(
        read_beneath(EMPTY),
        ERRNO_NOENT,
        "path_open of the empty path beneath a Mount through preview1 must answer ENOENT"
    );
}

// @behavior RT-087
#[test]
fn a_dot_dot_after_a_missing_name_names_nothing() {
    assert_eq!(
        read_beneath(THROUGH_MISSING),
        ERRNO_NOENT,
        "path_open of `missing/../file.txt` beneath a Mount through preview1 must answer ENOENT"
    );
}

// @behavior RT-088
#[test]
fn a_dot_dot_after_a_file_is_not_a_directory() {
    assert_eq!(
        read_beneath(OUT_OF_FILE),
        ERRNO_NOTDIR,
        "path_open of `file.txt/..` beneath a Mount through preview1 must answer ENOTDIR"
    );
}

// @behavior RT-089
#[test]
fn a_path_ending_in_a_slash_names_only_a_directory() {
    assert_eq!(
        read_beneath(FILE_AS_DIRECTORY),
        ERRNO_NOTDIR,
        "path_open of `file.txt/` beneath a Mount through preview1 must answer ENOTDIR"
    );
}

// The target is the source's own path, so only the slash can refuse it.
// @behavior RT-093
#[test]
fn a_rename_of_a_file_onto_a_path_ending_in_a_slash_is_not_a_directory() {
    let memory: Arc<Memory> = Arc::new([("file.txt", "granted")].into_iter().collect());
    let mut guest = Guest::new(&[grant(memory.clone(), Access::ReadWrite, None)]);
    assert_eq!(
        (
            guest.call_bare("rename_onto_itself_as_directory"),
            contents(memory.as_ref(), "file.txt")
        ),
        (ERRNO_NOTDIR, Some(b"granted".to_vec())),
        "path_rename of `file.txt` onto `file.txt/` beneath a Mount through preview1 must \
         answer ENOTDIR and leave the file"
    );
}

// @behavior RT-090
#[test]
fn a_file_created_at_a_path_ending_in_a_slash_is_refused_as_a_directory() {
    let memory = Arc::new(Memory::new());
    let mut guest = Guest::new(&[grant(memory.clone(), Access::ReadWrite, None)]);
    assert_eq!(
        (
            guest.call("write_file", NEW_AS_DIRECTORY),
            memory.read_dir("").map(|entries| entries.len())
        ),
        (ERRNO_ISDIR, Ok(0)),
        "path_open creating `new.txt/` beneath a Mount through preview1 must answer EISDIR and \
         make no file"
    );
}

const OFLAGS_CREAT: i32 = 1 << 0;
const OFLAGS_DIRECTORY: i32 = 1 << 1;
const OFLAGS_TRUNC: i32 = 1 << 3;
const FDFLAGS_SYNC: i32 = 1 << 4;
const ERRNO_NOTSUP: i32 = 58;

/// What opening `path` with `oflags` and `fdflags` answers beneath a
/// read-write Memory holding `file.txt`, and the paths it holds after.
fn open_flagged(path: (i32, i32), oflags: i32, fdflags: i32) -> (i32, Vec<String>) {
    let memory: Arc<Memory> = Arc::new([("file.txt", "granted")].into_iter().collect());
    let errno = Guest::new(&[grant(memory.clone(), Access::ReadWrite, None)])
        .call_flagged(path, oflags, fdflags);
    let held = memory
        .read_dir("")
        .expect("the root must list")
        .into_iter()
        .map(|entry| entry.name)
        .collect();
    (errno, held)
}

// @behavior RT-091
#[test]
fn an_open_asking_for_synchronized_writes_is_unsupported() {
    assert_eq!(
        open_flagged(FILE, 0, FDFLAGS_SYNC).0,
        ERRNO_NOTSUP,
        "path_open of `file.txt` with FDFLAGS_SYNC beneath a Mount through preview1 must answer \
         ENOTSUP"
    );
}

// @behavior RT-092
#[test]
fn an_open_of_a_directory_that_creates_or_truncates_is_invalid() {
    let held = vec!["file.txt".to_string()];
    assert_eq!(
        (
            open_flagged(NEW_AS_DIRECTORY, OFLAGS_DIRECTORY | OFLAGS_CREAT, 0),
            open_flagged(FILE, OFLAGS_DIRECTORY | OFLAGS_TRUNC, 0)
        ),
        ((ERRNO_INVAL, held.clone()), (ERRNO_INVAL, held)),
        "path_open with OFLAGS_DIRECTORY and CREAT or TRUNC beneath a Mount through preview1 \
         must answer EINVAL and make nothing"
    );
}
