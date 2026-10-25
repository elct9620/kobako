//! The differential probe beneath a Mount: each path call, made on the
//! same host directory through kobako's `Directory` Mount and through
//! wasmtime-wasi's own preopen, must leave the same errno, the same
//! descriptor, and the same directory behind. A listing of the Mount must
//! answer the same errno.
//!
//! A Filesystem reports no timestamps or inode numbers, so of what a
//! stat writes only the filetype is compared; the errno and the
//! directory carry the rest of the decision.
//!
//! Where POSIX lets each host choose, wasmtime-wasi passes on the host's
//! own answer — unlinking a directory is `ISDIR` on Linux and `PERM` on
//! macOS. kobako answers one way on every host, the way Linux does, so
//! the probe holds where CI runs it.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};

use kobako_runtime::filesystem::Access;
use kobako_runtime::filesystem::Directory;
use kobako_runtime::grant::Grant;
use kobako_runtime::mount::Mount;
use kobako_runtime::profile::Profile;
use wasmtime::{Linker, Module, Store};
use wasmtime_wasi::p1::{self, WasiP1Ctx};
use wasmtime_wasi::{FsPerms, WasiCtxBuilder};

use crate::cache::shared_engine;
use crate::config::Config;
use crate::imports;
use crate::invocation::Invocation;
use crate::preview1::beneath::PATH_MAX;
use crate::trap::NO_TIMEOUT_EPOCH_DELTA;
use crate::wasi::Wasi;

/// The guest path both sides preopen the host directory at.
const AT: &str = "/data";

/// Each export calls one path import on descriptor 3 with the path the
/// host placed at 1024, and the second path at 8192 where a call takes
/// two. An opened descriptor lands at 128, and a stat at 256.
const PROBE_WAT: &str = r#"
(module
  (import "wasi_snapshot_preview1" "path_open" (func $open (param i32 i32 i32 i32 i32 i64 i64 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "path_filestat_get" (func $stat (param i32 i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "path_create_directory" (func $mkdir (param i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "path_remove_directory" (func $rmdir (param i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "path_unlink_file" (func $unlink (param i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "path_rename" (func $rename (param i32 i32 i32 i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (func (export "open") (param $len i32) (param $oflags i32) (param $rights i64) (param $fdflags i32) (result i32)
    (call $open (i32.const 3) (i32.const 1) (i32.const 1024) (local.get $len) (local.get $oflags)
      (local.get $rights) (i64.const 0) (local.get $fdflags) (i32.const 128)))
  (func (export "open_nofollow") (param $len i32) (result i32)
    (call $open (i32.const 3) (i32.const 0) (i32.const 1024) (local.get $len) (i32.const 0)
      (i64.const 2) (i64.const 0) (i32.const 0) (i32.const 128)))
  (func (export "stat") (param $len i32) (result i32)
    (call $stat (i32.const 3) (i32.const 1) (i32.const 1024) (local.get $len) (i32.const 256)))
  (func (export "stat_nofollow") (param $len i32) (result i32)
    (call $stat (i32.const 3) (i32.const 0) (i32.const 1024) (local.get $len) (i32.const 256)))
  (func (export "mkdir") (param $len i32) (result i32)
    (call $mkdir (i32.const 3) (i32.const 1024) (local.get $len)))
  (func (export "rmdir") (param $len i32) (result i32)
    (call $rmdir (i32.const 3) (i32.const 1024) (local.get $len)))
  (func (export "unlink") (param $len i32) (result i32)
    (call $unlink (i32.const 3) (i32.const 1024) (local.get $len)))
  (func (export "rename") (param $len i32) (param $to i32) (result i32)
    (call $rename (i32.const 3) (i32.const 1024) (local.get $len) (i32.const 3) (i32.const 8192) (local.get $to))))
"#;

const PATH_AT: usize = 1024;
const SECOND_PATH_AT: usize = 8192;
const OPENED_AT: usize = 128;
/// Where a stat's filetype lands, past its device and inode.
const FILETYPE_AT: usize = 256 + 16;
const FILETYPE_DIRECTORY: u8 = 3;

const OFLAGS_CREAT: i32 = 1 << 0;
const OFLAGS_DIRECTORY: i32 = 1 << 1;
const OFLAGS_EXCL: i32 = 1 << 2;
const OFLAGS_TRUNC: i32 = 1 << 3;
const FDFLAGS_SYNC: i32 = 1 << 4;
const RIGHTS_FD_READ: i64 = 1 << 1;
const RIGHTS_FD_WRITE: i64 = 1 << 6;

/// Paths that stay beneath the root, climb above it, pass through what
/// does not exist or is no directory, or name the root itself.
const PATHS: &[&str] = &[
    "a",
    "l",
    "missing",
    "",
    ".",
    "a/",
    "l/",
    "n/",
    "d",
    "d/f",
    "d/..",
    "d/../a",
    "a/..",
    "missing/..",
    "missing/../a",
    "../a",
    "d/../../a",
    "/a",
    "n/..",
    "n/../f",
    "n/.",
];

/// Paths at Linux's limits: a name of `NAME_MAX` bytes and one past it,
/// and a path that fits `PATH_MAX` with its terminating NUL and one that
/// does not.
fn paths_at_the_limits() -> Vec<String> {
    let nested = |len: usize| {
        let mut path = "d/".repeat(len / 2);
        path.truncate(len - 1);
        path.push('x');
        path
    };
    vec![
        "x".repeat(255),
        "x".repeat(256),
        format!("d/{}", "x".repeat(256)),
        nested(4095),
        nested(4096),
    ]
}

/// One path call, as the probe makes it.
#[derive(Clone, Copy, Debug)]
enum Call {
    Open {
        oflags: i32,
        rights: i64,
        fdflags: i32,
    },
    /// Open the path to read, following no symbolic link at its end.
    OpenNoFollow,
    Stat,
    /// Stat the path, following no symbolic link at its end.
    StatNoFollow,
    Mkdir,
    Rmdir,
    Unlink,
    /// Rename the path to `b`.
    RenameFrom,
    /// Rename `a` to the path.
    RenameTo,
}

const CALLS: &[Call] = &[
    Call::Open {
        oflags: 0,
        rights: RIGHTS_FD_READ,
        fdflags: 0,
    },
    Call::Open {
        oflags: 0,
        rights: RIGHTS_FD_WRITE,
        fdflags: 0,
    },
    Call::Open {
        oflags: OFLAGS_CREAT | OFLAGS_TRUNC,
        rights: RIGHTS_FD_WRITE,
        fdflags: 0,
    },
    Call::Open {
        oflags: OFLAGS_CREAT | OFLAGS_EXCL,
        rights: RIGHTS_FD_WRITE,
        fdflags: 0,
    },
    Call::Open {
        oflags: OFLAGS_DIRECTORY,
        rights: RIGHTS_FD_READ,
        fdflags: 0,
    },
    Call::Open {
        oflags: OFLAGS_DIRECTORY | OFLAGS_CREAT,
        rights: RIGHTS_FD_READ,
        fdflags: 0,
    },
    Call::Open {
        oflags: OFLAGS_DIRECTORY | OFLAGS_TRUNC,
        rights: RIGHTS_FD_READ,
        fdflags: 0,
    },
    Call::Open {
        oflags: 0,
        rights: RIGHTS_FD_READ,
        fdflags: FDFLAGS_SYNC,
    },
    Call::OpenNoFollow,
    Call::Stat,
    Call::StatNoFollow,
    Call::Mkdir,
    Call::Rmdir,
    Call::Unlink,
    Call::RenameFrom,
    Call::RenameTo,
];

/// A host directory of the probe's own, holding `a`, `d/f`, `d/e/`, a
/// link `l` to `a`, and a link `n` to `d/e`, whose parent is not the
/// root, removed when it drops.
struct HostDir(PathBuf);

impl HostDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "kobako-differential-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(path.join("d/e")).expect("the host directory must be created");
        fs::write(path.join("a"), "hello").expect("the host file must be written");
        fs::write(path.join("d/f"), "inner").expect("the host file must be written");
        #[cfg(unix)]
        for (target, link) in [("a", "l"), ("d/e", "n")] {
            std::os::unix::fs::symlink(target, path.join(link))
                .expect("the host link must be made");
        }
        Self(path)
    }

    /// Every entry beneath the directory, with a file's content.
    fn tree(&self) -> Vec<(String, Option<Vec<u8>>)> {
        fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, Option<Vec<u8>>)>) {
            for entry in fs::read_dir(dir).expect("the host directory must list") {
                let path = entry.expect("the host entry must read").path();
                let name = path
                    .strip_prefix(root)
                    .expect("an entry lies beneath its root")
                    .to_string_lossy()
                    .into_owned();
                if path.is_symlink() {
                    let target = fs::read_link(&path).expect("the host link must read");
                    out.push((
                        name,
                        Some(target.to_string_lossy().into_owned().into_bytes()),
                    ));
                } else if path.is_dir() {
                    out.push((name, None));
                    walk(root, &path, out);
                } else {
                    out.push((
                        name,
                        Some(fs::read(&path).expect("the host file must read")),
                    ));
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.0, &self.0, &mut out);
        out.sort();
        out
    }
}

impl Drop for HostDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// What one call leaves behind; `None` for a call that trapped.
#[derive(Debug, PartialEq)]
struct Observation {
    errno: Option<i32>,
    opened: [u8; 4],
    filetype: u8,
    tree: Vec<(String, Option<Vec<u8>>)>,
}

fn observe<T>(
    mut store: Store<T>,
    linker: &Linker<T>,
    call: Call,
    path: &str,
    host: &HostDir,
) -> Observation {
    static MODULE: OnceLock<Module> = OnceLock::new();
    let module = MODULE
        .get_or_init(|| Module::new(store.engine(), PROBE_WAT).expect("probe module must compile"));
    let instance = linker
        .instantiate(&mut store, module)
        .expect("probe module must instantiate");
    let memory = instance
        .get_memory(&mut store, "memory")
        .expect("probe exports its memory");
    let (first, second) = match call {
        Call::RenameTo => ("a", path),
        Call::RenameFrom => (path, "b"),
        _ => (path, ""),
    };
    memory.data_mut(&mut store)[PATH_AT..PATH_AT + first.len()].copy_from_slice(first.as_bytes());
    memory.data_mut(&mut store)[SECOND_PATH_AT..SECOND_PATH_AT + second.len()]
        .copy_from_slice(second.as_bytes());
    let len = first.len() as i32;
    let errno = match call {
        Call::Open {
            oflags,
            rights,
            fdflags,
        } => instance
            .get_typed_func::<(i32, i32, i64, i32), i32>(&mut store, "open")
            .and_then(|f| f.call(&mut store, (len, oflags, rights, fdflags))),
        Call::RenameFrom | Call::RenameTo => instance
            .get_typed_func::<(i32, i32), i32>(&mut store, "rename")
            .and_then(|f| f.call(&mut store, (len, second.len() as i32))),
        Call::OpenNoFollow
        | Call::Stat
        | Call::StatNoFollow
        | Call::Mkdir
        | Call::Rmdir
        | Call::Unlink => {
            let name = match call {
                Call::OpenNoFollow => "open_nofollow",
                Call::Stat => "stat",
                Call::StatNoFollow => "stat_nofollow",
                Call::Mkdir => "mkdir",
                Call::Rmdir => "rmdir",
                _ => "unlink",
            };
            instance
                .get_typed_func::<i32, i32>(&mut store, name)
                .and_then(|f| f.call(&mut store, len))
        }
    }
    .ok();
    let mut opened = [0; 4];
    opened.copy_from_slice(&memory.data(&store)[OPENED_AT..OPENED_AT + 4]);
    Observation {
        errno,
        opened,
        filetype: memory.data(&store)[FILETYPE_AT],
        tree: host.tree(),
    }
}

fn through_kobako(call: Call, path: &str) -> Observation {
    let host = HostDir::new();
    let (store, linker) = kobako_preopen(&host);
    observe(store, &linker, call, path, &host)
}

fn through_wasmtime_wasi(call: Call, path: &str) -> Observation {
    let host = HostDir::new();
    let (store, linker) = wasmtime_wasi_preopen(&host);
    observe(store, &linker, call, path, &host)
}

/// `host` behind a kobako `Directory` Mount at `AT`.
fn kobako_preopen(host: &HostDir) -> (Store<Invocation>, Linker<Invocation>) {
    let engine = shared_engine().expect("shared engine must be constructible");
    let config = Config {
        timeout: None,
        setup_timeout: None,
        memory_limit: None,
        stdout_limit: None,
        stderr_limit: None,
        open_file_limit: None,
        profile: Profile::Hermetic,
    };
    let directory = Directory::new(&host.0).expect("the host directory must open");
    let grant = Grant::Mount(
        Mount::new(AT, Arc::new(directory), Access::ReadWrite).expect("AT is a guest path"),
    );
    let wasi = Wasi::for_invocation(&config, &[], &[grant]).expect("WASI context must build");
    let mut store = Store::new(engine, Invocation::new(None, wasi));
    store.set_epoch_deadline(NO_TIMEOUT_EPOCH_DELTA);
    let linker = imports::linker().expect("host imports must link");
    (store, linker)
}

/// `host` behind wasmtime-wasi's own preopen at `AT`.
fn wasmtime_wasi_preopen(host: &HostDir) -> (Store<WasiP1Ctx>, Linker<WasiP1Ctx>) {
    let engine = shared_engine().expect("shared engine must be constructible");
    let ctx = WasiCtxBuilder::new()
        .preopened_dir(&host.0, AT, FsPerms::ReadWrite)
        .expect("the host directory must preopen")
        .build_p1();
    let mut store = Store::new(engine, ctx);
    store.set_epoch_deadline(NO_TIMEOUT_EPOCH_DELTA);
    let mut linker = Linker::new(engine);
    p1::add_to_linker_sync(&mut linker, |ctx: &mut WasiP1Ctx| ctx).expect("preview1 must link");
    (store, linker)
}

const ERRNO_BUSY: i32 = 10;
const ERRNO_INVAL: i32 = 28;
const ERRNO_LOOP: i32 = 32;
const ERRNO_NAMETOOLONG: i32 = 37;
const ERRNO_NOTDIR: i32 = 54;
const ERRNO_NOTSUP: i32 = 58;
const ERRNO_PERM: i32 = 63;
const ERRNO_NOTCAPABLE: i32 = 76;

/// Whether `path` reaches above the root it is resolved beneath.
/// Whether `path` takes a `..`, or ends in a `.`, right after one of the
/// host directory's links.
fn steps_out_of_a_link(path: &str) -> bool {
    let mut components = path.split('/');
    matches!(components.next(), Some("l" | "n"))
        && match components.next() {
            Some("..") => true,
            Some(".") => components.next().is_none(),
            _ => false,
        }
}

fn escapes(path: &str) -> bool {
    let mut depth = 0usize;
    path.starts_with('/')
        || path.split('/').any(|component| match component {
            "" | "." => false,
            ".." => match depth.checked_sub(1) {
                Some(up) => {
                    depth = up;
                    false
                }
                None => true,
            },
            _ => {
                depth += 1;
                false
            }
        })
}

/// What kobako answers where it differs from wasmtime-wasi on purpose. A
/// path above the root is `NOTCAPABLE`, the code preview1 names for a
/// capability the guest was never handed, where wasmtime-wasi passes on
/// cap-std's refusal as `PERM`. A path past `PATH_MAX` is `NAMETOOLONG`
/// as Linux answers the whole path, where cap-std resolves it one name
/// at a time and never hands Linux the whole of it. A stat following no
/// link of a path named with a trailing `/` answers as Linux's `lstat`,
/// which follows a link there, where cap-std's shortcut for one name
/// drops the `/` and finds the file or the link. A `rmdir` of a link
/// named with a trailing `/` is `NOTDIR` as Linux answers it, where
/// cap-std's split turns the `/` into `.` and answers `INVAL`. A `..` or
/// a final `.` after a link is `LOOP` and changes nothing, where cap-std
/// steps out of where the link points; a call refused before its path
/// resolves answers alike.
fn expected(call: Call, path: &str, mut oracle: Observation) -> Observation {
    let refused_first = matches!(oracle.errno, Some(ERRNO_BUSY | ERRNO_INVAL | ERRNO_NOTSUP));
    if steps_out_of_a_link(path) && !refused_first {
        return Observation {
            errno: Some(ERRNO_LOOP),
            opened: [0; 4],
            filetype: 0,
            tree: HostDir::new().tree(),
        };
    }
    if escapes(path) && oracle.errno == Some(ERRNO_PERM) {
        oracle.errno = Some(ERRNO_NOTCAPABLE);
    }
    if matches!(call, Call::StatNoFollow) && path.ends_with('/') && oracle.errno == Some(0) {
        let host = HostDir::new();
        match fs::symlink_metadata(host.0.join(path)) {
            Ok(metadata) if metadata.is_dir() => oracle.filetype = FILETYPE_DIRECTORY,
            _ => {
                oracle.errno = Some(ERRNO_NOTDIR);
                oracle.filetype = 0;
            }
        }
    }
    if matches!(call, Call::Rmdir)
        && matches!(path, "l/" | "n/")
        && oracle.errno == Some(ERRNO_INVAL)
    {
        oracle.errno = Some(ERRNO_NOTDIR);
    }
    if path.len() >= PATH_MAX {
        oracle.errno = Some(ERRNO_NAMETOOLONG);
    }
    oracle
}

#[test]
#[cfg_attr(
    not(target_os = "linux"),
    ignore = "wasmtime-wasi passes on the host's own answer, and kobako answers as Linux does"
)]
fn every_path_call_beneath_a_mount_answers_as_a_wasmtime_wasi_preopen() {
    let paths: Vec<String> = PATHS
        .iter()
        .map(|&path| path.to_string())
        .chain(paths_at_the_limits())
        .collect();
    let differing: Vec<String> = CALLS
        .iter()
        .flat_map(|&call| paths.iter().map(move |path| (call, path.as_str())))
        .filter_map(|(call, path)| {
            let kobako = through_kobako(call, path);
            let oracle = expected(call, path, through_wasmtime_wasi(call, path));
            (kobako != oracle).then(|| format!("{call:?} {path:?}: {kobako:?} != {oracle:?}"))
        })
        .collect();
    assert!(
        differing.is_empty(),
        "each path call on a host directory through a kobako Directory Mount must answer as \
         wasmtime-wasi's preopen of it does:\n{}",
        differing.join("\n")
    );
}

// Only Linux keeps a name that is not UTF-8; macOS refuses to make one.
#[test]
#[cfg(target_os = "linux")]
fn a_listing_holding_a_name_that_is_not_utf8_answers_as_a_wasmtime_wasi_preopen() {
    use std::os::unix::ffi::OsStrExt;

    /// Lists descriptor 3 into the 256 bytes at 1024 from the first entry,
    /// the bytes used landing at 128.
    const READDIR_WAT: &str = r#"
    (module
      (import "wasi_snapshot_preview1" "fd_readdir" (func $readdir (param i32 i32 i32 i64 i32) (result i32)))
      (memory (export "memory") 1)
      (func (export "readdir") (result i32)
        (call $readdir (i32.const 3) (i32.const 1024) (i32.const 256) (i64.const 0) (i32.const 128))))
    "#;

    /// The errno the listing answers; `None` for a call that trapped.
    fn readdir_errno<T>(mut store: Store<T>, linker: &Linker<T>) -> Option<i32> {
        let module = Module::new(store.engine(), READDIR_WAT).expect("probe module must compile");
        let instance = linker
            .instantiate(&mut store, &module)
            .expect("probe module must instantiate");
        instance
            .get_typed_func::<(), i32>(&mut store, "readdir")
            .and_then(|f| f.call(&mut store, ()))
            .ok()
    }

    let host = HostDir::new();
    fs::write(host.0.join(std::ffi::OsStr::from_bytes(b"\xff")), "")
        .expect("the host file must be written");
    let (store, linker) = kobako_preopen(&host);
    let kobako = readdir_errno(store, &linker);
    let (store, linker) = wasmtime_wasi_preopen(&host);
    let oracle = readdir_errno(store, &linker);
    assert_eq!(
        kobako, oracle,
        "fd_readdir on a host directory holding a name that is not UTF-8 through a kobako \
         Directory Mount must answer as wasmtime-wasi's preopen of it does"
    );
}
