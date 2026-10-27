//! Witness what a guest sees through a Mount: the preopen at the Mount's
//! path, the files a Filesystem holds, and the refusals the runtime
//! answers before the Filesystem is asked.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use kobako_runtime::filesystem::Access;
use kobako_runtime::filesystem::{
    DirEntry, Directory, File, Filesystem, FilesystemError, Hold, Memory, Metadata, OpenOptions,
    Overlay, Rename,
};
use kobako_runtime::grant::Grant;
use kobako_runtime::mount::{Mount, Policy};
use kobako_runtime::profile::Profile;
use wasmtime::{Instance, Module, Store};

use crate::cache::shared_engine;
use crate::config::Config;
use crate::imports;
use crate::invocation::Invocation;
use crate::trap::NO_TIMEOUT_EPOCH_DELTA;
use crate::wasi::Wasi;

/// The answer of a test Filesystem holding no links.
macro_rules! holds_no_links {
    () => {
        fn symlink_metadata(&self, path: &str) -> Result<Metadata, FilesystemError> {
            self.metadata(path)
        }

        fn canonicalize_beneath(&self, _base: &str, path: &str) -> Result<String, FilesystemError> {
            Ok(path.to_string())
        }

        fn for_policy(&self) -> Option<Arc<dyn Filesystem>> {
            None
        }
    };
}

/// The answer of a test Filesystem matching names byte for byte.
macro_rules! matches_names_exactly {
    () => {
        fn stored_name(&self, name: &str) -> Result<String, FilesystemError> {
            Ok(name.to_string())
        }
    };
}

/// The answers of a test Filesystem over the Memory in `$inner` that opens
/// and lists its directories as that Memory does.
macro_rules! opens_directories {
    ($inner:tt) => {
        fn open_dir(&self, path: &str) -> Result<Arc<dyn Filesystem>, FilesystemError> {
            self.$inner.open_dir(path)
        }

        fn open_dir_nofollow(&self, name: &str) -> Result<Arc<dyn Filesystem>, FilesystemError> {
            self.$inner.open_dir(name)
        }

        fn entries(&self) -> Result<Vec<DirEntry>, FilesystemError> {
            self.$inner.entries()
        }
    };
}

/// The answer of a test Filesystem whose identities never repeat.
macro_rules! never_repeats_identities {
    () => {
        fn hold_directory(&self, _path: &str) -> Result<Option<Hold>, FilesystemError> {
            Ok(None)
        }
    };
}

const ERRNO_ACCES: i32 = 2;
const ERRNO_BADF: i32 = 8;
const ERRNO_BUSY: i32 = 10;
const ERRNO_FBIG: i32 = 22;
const ERRNO_INVAL: i32 = 28;
const ERRNO_IO: i32 = 29;
const ERRNO_ISDIR: i32 = 31;
const ERRNO_LOOP: i32 = 32;
const ERRNO_NOENT: i32 = 44;
const ERRNO_NOTDIR: i32 = 54;
const ERRNO_NOTSUP: i32 = 58;
const ERRNO_ROFS: i32 = 69;
const ERRNO_XDEV: i32 = 75;
const ERRNO_NOTCAPABLE: i32 = 76;

/// Memory holds an `iovec` at 64 over 512..576, a `ciovec` at 80 over
/// "hello" at 600, the paths `file.txt` at 700 and `../file.txt` at 720,
/// and from 740 the paths the resolution rules turn on. An opened
/// descriptor lands at 128, a count at 132, a listing's count at 136, a
/// stat at 384, and a listing at 2048.
const PROBE_WAT: &str = r#"
(module
  (import "wasi_snapshot_preview1" "fd_prestat_get" (func $prestat (param i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_prestat_dir_name" (func $prestat_name (param i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "path_open" (func $open (param i32 i32 i32 i32 i32 i64 i64 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "path_filestat_get" (func $stat (param i32 i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_read" (func $read (param i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_write" (func $write (param i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "path_remove_directory" (func $rmdir (param i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "path_rename" (func $rename (param i32 i32 i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "path_unlink_file" (func $unlink (param i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_allocate" (func $allocate (param i32 i64 i64) (result i32)))
  (import "wasi_snapshot_preview1" "fd_seek" (func $seek (param i32 i64 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_readdir" (func $readdir (param i32 i32 i32 i64 i32) (result i32)))
  (import "wasi_snapshot_preview1" "path_create_directory" (func $mkdir (param i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_pread" (func $pread (param i32 i32 i32 i64 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_pwrite" (func $pwrite (param i32 i32 i32 i64 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_filestat_set_size" (func $set_size (param i32 i64) (result i32)))
  (import "wasi_snapshot_preview1" "fd_advise" (func $advise (param i32 i64 i64 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_fdstat_get" (func $fdstat (param i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_fdstat_set_flags" (func $set_flags (param i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "poll_oneoff" (func $poll (param i32 i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 740) ".")
  (data (i32.const 760) "missing/../file.txt")
  (data (i32.const 780) "file.txt/..")
  (data (i32.const 800) "file.txt/")
  (data (i32.const 820) "new.txt/")
  (data (i32.const 840) "new.txt")
  (data (i32.const 860) "file.txt/key")
  (data (i32.const 880) "dir/.")
  (data (i32.const 890) "dir/..")
  (data (i32.const 900) "file.txt/x")
  (data (i32.const 920) "moved")
  (data (i32.const 930) "dir")
  (data (i32.const 940) "missing/.")
  (data (i32.const 960) "inner")
  (data (i32.const 970) "file.txt/.")
  (data (i32.const 990) "file.txt/../file.txt")
  (data (i32.const 1010) "file.txt\00")
  (data (i32.const 1030) "dir/file.txt")
  (data (i32.const 1050) "dir/")
  (data (i32.const 1060) "gone/")
  (data (i32.const 1070) "up")
  (data (i32.const 1080) "updir/x")
  (data (i32.const 1090) "in/y")
  (data (i32.const 1100) "twisty")
  (data (i32.const 1110) "dangle")
  (data (i32.const 1120) "dir/up")
  (data (i32.const 1130) "nowhere")
  (data (i32.const 1150) "pub/x")
  ;; An iovec naming 16 bytes past the one page of memory.
  (data (i32.const 1140) "\00\00\02\00\10\00\00\00")
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

  ;; Stat the path with the given lookupflags into 384.
  (func (export "stat_flagged") (param $path i32) (param $len i32) (param $flags i32) (result i32)
    (call $stat (i32.const 3) (local.get $flags) (local.get $path) (local.get $len) (i32.const 384)))

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

  ;; Open the path to write and allocate 100 bytes from its start.
  (func (export "allocate_file") (param $path i32) (param $len i32) (result i32)
    (local $errno i32)
    (local.set $errno (call $open (i32.const 3) (i32.const 0) (local.get $path) (local.get $len)
      (i32.const 0) (i64.const 64) (i64.const 0) (i32.const 0) (i32.const 128)))
    (if (local.get $errno) (then (return (local.get $errno))))
    (call $allocate (i32.load (i32.const 128)) (i64.const 0) (i64.const 100)))

  ;; Rename `file.txt` to `new.txt`.
  (func (export "rename_to_new") (result i32)
    (call $rename (i32.const 3) (i32.const 700) (i32.const 8) (i32.const 3) (i32.const 840) (i32.const 7)))
  ;; Rename `file.txt` onto `file.txt/`, its own path as a directory.
  (func (export "rename_onto_itself_as_directory") (result i32)
    (call $rename (i32.const 3) (i32.const 700) (i32.const 8) (i32.const 3) (i32.const 800) (i32.const 9)))
  (func (export "unlink_root") (result i32)
    (call $unlink (i32.const 3) (i32.const 740) (i32.const 1)))
  (func (export "unlink_path") (param $path i32) (param $len i32) (result i32)
    (call $unlink (i32.const 3) (local.get $path) (local.get $len)))
  (func (export "remove_path") (param $path i32) (param $len i32) (result i32)
    (call $rmdir (i32.const 3) (local.get $path) (local.get $len)))
  (func (export "make_dir") (param $path i32) (param $len i32) (result i32)
    (call $mkdir (i32.const 3) (local.get $path) (local.get $len)))
  (func (export "rename_paths") (param $from i32) (param $from_len i32) (param $to i32) (param $to_len i32) (result i32)
    (call $rename (i32.const 3) (local.get $from) (local.get $from_len) (i32.const 3) (local.get $to) (local.get $to_len)))
  ;; Open `file.txt`, seek it to the largest offset, then that far again.
  (func (export "seek_past_the_largest_offset") (result i32)
    (local $errno i32)
    (local.set $errno (call $open (i32.const 3) (i32.const 0) (i32.const 700) (i32.const 8)
      (i32.const 0) (i64.const 2) (i64.const 0) (i32.const 0) (i32.const 128)))
    (if (local.get $errno) (then (return (local.get $errno))))
    (local.set $errno (call $seek (i32.load (i32.const 128)) (i64.const 0x7fffffffffffffff) (i32.const 0) (i32.const 136)))
    (if (local.get $errno) (then (return (local.get $errno))))
    (call $seek (i32.load (i32.const 128)) (i64.const 0x7fffffffffffffff) (i32.const 1) (i32.const 136)))
  ;; Open `file.txt` to read and write into 128.
  (func $open_read_write (result i32)
    (call $open (i32.const 3) (i32.const 0) (i32.const 700) (i32.const 8)
      (i32.const 0) (i64.const 66) (i64.const 0) (i32.const 0) (i32.const 128)))
  ;; Read `file.txt` into 512 at `offset`.
  (func (export "read_at") (param $offset i64) (result i32)
    (local $errno i32)
    (local.set $errno (call $open_read_write))
    (if (local.get $errno) (then (return (local.get $errno))))
    (call $pread (i32.load (i32.const 128)) (i32.const 64) (i32.const 1) (local.get $offset) (i32.const 132)))
  ;; Write "hello" into `file.txt` at `offset`.
  (func (export "write_at") (param $offset i64) (result i32)
    (local $errno i32)
    (local.set $errno (call $open_read_write))
    (if (local.get $errno) (then (return (local.get $errno))))
    (call $pwrite (i32.load (i32.const 128)) (i32.const 80) (i32.const 1) (local.get $offset) (i32.const 132)))
  ;; Open `file.txt`, seek it to ten before the largest offset, and read
  ;; the 64 bytes the iovec at 64 holds.
  (func (export "read_near_the_largest_offset") (result i32)
    (local $errno i32)
    (local.set $errno (call $open_read_write))
    (if (local.get $errno) (then (return (local.get $errno))))
    (local.set $errno (call $seek (i32.load (i32.const 128)) (i64.const 0x7ffffffffffffff5) (i32.const 0) (i32.const 136)))
    (if (local.get $errno) (then (return (local.get $errno))))
    (call $read (i32.load (i32.const 128)) (i32.const 64) (i32.const 1) (i32.const 132)))
  ;; Open `file.txt` with `rights`, then wait to write to it with the one
  ;; subscription at 1200, its event landing at 1264.
  (func (export "poll_write_with") (param $rights i64) (result i32)
    (local $errno i32)
    (local.set $errno (call $open (i32.const 3) (i32.const 0) (i32.const 700) (i32.const 8)
      (i32.const 0) (local.get $rights) (i64.const 0) (i32.const 0) (i32.const 128)))
    (if (local.get $errno) (then (return (local.get $errno))))
    (i32.store8 (i32.const 1208) (i32.const 2))
    (i32.store (i32.const 1216) (i32.load (i32.const 128)))
    (call $poll (i32.const 1200) (i32.const 1264) (i32.const 1) (i32.const 1296)))
  ;; Open `file.txt`, seek it to the largest offset, and write "hello".
  (func (export "write_at_the_largest_offset") (result i32)
    (local $errno i32)
    (local.set $errno (call $open_read_write))
    (if (local.get $errno) (then (return (local.get $errno))))
    (local.set $errno (call $seek (i32.load (i32.const 128)) (i64.const 0x7fffffffffffffff) (i32.const 0) (i32.const 136)))
    (if (local.get $errno) (then (return (local.get $errno))))
    (call $write (i32.load (i32.const 128)) (i32.const 80) (i32.const 1) (i32.const 132)))
  ;; Advise on `file.txt` from `offset`, and for `offset` bytes from 0.
  (func (export "advise_from") (param $offset i64) (result i32)
    (local $errno i32)
    (local.set $errno (call $open_read_write))
    (if (local.get $errno) (then (return (local.get $errno))))
    (call $advise (i32.load (i32.const 128)) (local.get $offset) (i64.const 0) (i32.const 0)))
  (func (export "advise_for") (param $len i64) (result i32)
    (local $errno i32)
    (local.set $errno (call $open_read_write))
    (if (local.get $errno) (then (return (local.get $errno))))
    (call $advise (i32.load (i32.const 128)) (i64.const 0) (local.get $len) (i32.const 0)))
  ;; Open `file.txt`, then set it not to block.
  (func (export "set_nonblocking") (result i32)
    (local $errno i32)
    (local.set $errno (call $open_read_write))
    (if (local.get $errno) (then (return (local.get $errno))))
    (call $set_flags (i32.load (i32.const 128)) (i32.const 4)))
  ;; Open `dir` as a directory and read its fdstat into 384.
  (func (export "dir_fdstat") (result i32)
    (local $errno i32)
    (local.set $errno (call $open (i32.const 3) (i32.const 0) (i32.const 930) (i32.const 3)
      (i32.const 2) (i64.const 0) (i64.const 2) (i32.const 0) (i32.const 128)))
    (if (local.get $errno) (then (return (local.get $errno))))
    (call $fdstat (i32.load (i32.const 128)) (i32.const 384)))
  ;; Size `file.txt` to `offset` bytes.
  (func (export "size_at") (param $offset i64) (result i32)
    (local $errno i32)
    (local.set $errno (call $open_read_write))
    (if (local.get $errno) (then (return (local.get $errno))))
    (call $set_size (i32.load (i32.const 128)) (local.get $offset)))
  ;; Open `file.txt` with `rights`, then read or write it through the
  ;; iovec past memory at 1140.
  (func $open_with (param $rights i64) (result i32)
    (call $open (i32.const 3) (i32.const 0) (i32.const 700) (i32.const 8)
      (i32.const 0) (local.get $rights) (i64.const 0) (i32.const 0) (i32.const 128)))
  (func (export "read_far_with") (param $rights i64) (result i32)
    (local $errno i32)
    (local.set $errno (call $open_with (local.get $rights)))
    (if (local.get $errno) (then (return (local.get $errno))))
    (call $read (i32.load (i32.const 128)) (i32.const 1140) (i32.const 1) (i32.const 132)))
  (func (export "pread_far_with") (param $rights i64) (result i32)
    (local $errno i32)
    (local.set $errno (call $open_with (local.get $rights)))
    (if (local.get $errno) (then (return (local.get $errno))))
    (call $pread (i32.load (i32.const 128)) (i32.const 1140) (i32.const 1) (i64.const 0) (i32.const 132)))
  (func (export "write_far_with") (param $rights i64) (result i32)
    (local $errno i32)
    (local.set $errno (call $open_with (local.get $rights)))
    (if (local.get $errno) (then (return (local.get $errno))))
    (call $write (i32.load (i32.const 128)) (i32.const 1140) (i32.const 1) (i32.const 132)))
  (func (export "pwrite_far_with") (param $rights i64) (result i32)
    (local $errno i32)
    (local.set $errno (call $open_with (local.get $rights)))
    (if (local.get $errno) (then (return (local.get $errno))))
    (call $pwrite (i32.load (i32.const 128)) (i32.const 1140) (i32.const 1) (i64.const 0) (i32.const 132)))
  ;; Open `file.txt` with `rights`, then size it to one byte.
  (func (export "size_with") (param $rights i64) (result i32)
    (local $errno i32)
    (local.set $errno (call $open (i32.const 3) (i32.const 0) (i32.const 700) (i32.const 8)
      (i32.const 0) (local.get $rights) (i64.const 0) (i32.const 0) (i32.const 128)))
    (if (local.get $errno) (then (return (local.get $errno))))
    (call $set_size (i32.load (i32.const 128)) (i64.const 1)))
  ;; List the root from `cookie` into the `len` bytes at 2048.
  (func (export "list") (param $cookie i64) (param $len i32) (result i32)
    (call $readdir (i32.const 3) (i32.const 2048) (local.get $len) (local.get $cookie) (i32.const 136)))
  ;; Open `path` for reading, following a link standing at its end.
  (func (export "open_following") (param $path i32) (param $len i32) (result i32)
    (call $open (i32.const 3) (i32.const 1) (local.get $path) (local.get $len)
      (i32.const 0) (i64.const 2) (i64.const 0) (i32.const 0) (i32.const 128)))
  ;; Open `dir` as a directory, then the path beneath that descriptor.
  (func (export "open_beneath_dir") (param $path i32) (param $len i32) (result i32)
    (local $errno i32)
    (local.set $errno (call $open (i32.const 3) (i32.const 0) (i32.const 930) (i32.const 3)
      (i32.const 2) (i64.const 0) (i64.const 2) (i32.const 0) (i32.const 128)))
    (if (local.get $errno) (then (return (local.get $errno))))
    (call $open (i32.load (i32.const 128)) (i32.const 0) (local.get $path) (local.get $len)
      (i32.const 0) (i64.const 2) (i64.const 0) (i32.const 0) (i32.const 132)))
  ;; As `open_beneath_dir`, following a link at the path's end.
  (func (export "open_following_beneath_dir") (param $path i32) (param $len i32) (result i32)
    (local $errno i32)
    (local.set $errno (call $open_dir))
    (if (local.get $errno) (then (return (local.get $errno))))
    (call $open (i32.load (i32.const 128)) (i32.const 1) (local.get $path) (local.get $len)
      (i32.const 0) (i64.const 2) (i64.const 0) (i32.const 0) (i32.const 132)))
  ;; As `open_following_beneath_dir`, creating a file to write.
  (func (export "create_following_beneath_dir") (param $path i32) (param $len i32) (result i32)
    (local $errno i32)
    (local.set $errno (call $open_dir))
    (if (local.get $errno) (then (return (local.get $errno))))
    (call $open (i32.load (i32.const 128)) (i32.const 1) (local.get $path) (local.get $len)
      (i32.const 1) (i64.const 0x40) (i64.const 0) (i32.const 0) (i32.const 132)))
  ;; Stat the path beneath `dir`, following a link at its end, into 384.
  (func (export "stat_following_beneath_dir") (param $path i32) (param $len i32) (result i32)
    (local $errno i32)
    (local.set $errno (call $open_dir))
    (if (local.get $errno) (then (return (local.get $errno))))
    (call $stat (i32.load (i32.const 128)) (i32.const 1) (local.get $path) (local.get $len) (i32.const 384)))
  ;; Open `dir` as a directory into 128.
  (func $open_dir (export "open_dir") (result i32)
    (call $open (i32.const 3) (i32.const 0) (i32.const 930) (i32.const 3)
      (i32.const 2) (i64.const 0) (i64.const 2) (i32.const 0) (i32.const 128)))
  ;; Stat `inner` beneath the directory opened into 128.
  (func (export "stat_inner") (result i32)
    (call $stat (i32.load (i32.const 128)) (i32.const 0) (i32.const 960) (i32.const 5) (i32.const 384))))
"#;

const FILE: (i32, i32) = (700, 8);
const ABOVE_ROOT: (i32, i32) = (720, 11);
const EMPTY: (i32, i32) = (700, 0);
const THROUGH_MISSING: (i32, i32) = (760, 19);
const OUT_OF_FILE: (i32, i32) = (780, 11);
const FILE_AS_DIRECTORY: (i32, i32) = (800, 9);
const NEW_AS_DIRECTORY: (i32, i32) = (820, 8);
const THROUGH_FILE: (i32, i32) = (860, 12);
const DIR_DOT: (i32, i32) = (880, 5);
const DIR_DOT_DOT: (i32, i32) = (890, 6);
const BENEATH_FILE: (i32, i32) = (900, 10);
const MOVED: (i32, i32) = (920, 5);
const DIR: (i32, i32) = (930, 3);
const MISSING_DOT: (i32, i32) = (940, 9);
const LINK_DOT: (i32, i32) = (970, 10);
const OUT_OF_LINK: (i32, i32) = (990, 20);
const HOLDING_NUL: (i32, i32) = (1010, 9);
const DIR_FILE: (i32, i32) = (1030, 12);
const DIR_AS_DIRECTORY: (i32, i32) = (1050, 4);
const GONE_AS_DIRECTORY: (i32, i32) = (1060, 5);
const ERRNO_OVERFLOW: i32 = 61;
const ERRNO_EXIST: i32 = 20;
const OFLAGS_EXCL: i32 = 1 << 2;

/// The default `OpenOptions` with `set` applied.
fn build_options(set: impl FnOnce(&mut OpenOptions)) -> OpenOptions {
    let mut options = OpenOptions::default();
    set(&mut options);
    options
}

fn grant(
    filesystem: Arc<dyn Filesystem>,
    access: Access,
    policy: Option<Arc<dyn Policy>>,
) -> Grant {
    let mount = Mount::new("/data", filesystem, access).expect("/data is a guest path");
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
            setup_timeout: None,
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

    /// Call an export that takes two paths.
    fn call_pair(&mut self, name: &str, first: (i32, i32), second: (i32, i32)) -> i32 {
        self.instance
            .get_typed_func::<(i32, i32, i32, i32), i32>(&mut self.store, name)
            .expect("probe export must resolve")
            .call(&mut self.store, (first.0, first.1, second.0, second.1))
            .expect("probe call must not trap")
    }

    /// Call an export that takes an offset, `None` when it traps.
    fn try_call_at(&mut self, name: &str, offset: u64) -> Option<i32> {
        self.instance
            .get_typed_func::<i64, i32>(&mut self.store, name)
            .expect("probe export must resolve")
            .call(&mut self.store, offset as i64)
            .ok()
    }

    /// Call an export that takes an offset.
    fn call_at(&mut self, name: &str, offset: u64) -> i32 {
        self.instance
            .get_typed_func::<i64, i32>(&mut self.store, name)
            .expect("probe export must resolve")
            .call(&mut self.store, offset as i64)
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

    /// The names a listing from `cookie` into `len` bytes holds whole.
    fn list(&mut self, cookie: u64, len: i32) -> Vec<String> {
        let errno = self
            .instance
            .get_typed_func::<(i64, i32), i32>(&mut self.store, "list")
            .expect("probe export must resolve")
            .call(&mut self.store, (cookie as i64, len))
            .expect("probe call must not trap");
        assert_eq!(errno, 0, "the listing must succeed");
        let memory = self
            .instance
            .get_memory(&mut self.store, "memory")
            .expect("probe exports its memory");
        let data = memory.data(&self.store);
        let used = u32::from_le_bytes(data[136..140].try_into().unwrap()) as usize;
        let listed = &data[2048..2048 + used];
        let mut names = Vec::new();
        let mut at = 0;
        while at + 24 <= listed.len() {
            let len = u32::from_le_bytes(listed[at + 16..at + 20].try_into().unwrap()) as usize;
            let Some(name) = listed.get(at + 24..at + 24 + len) else {
                break;
            };
            names.push(String::from_utf8(name.to_vec()).expect("a listed name is UTF-8"));
            at += 24 + len;
        }
        names
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
        .open(path, &build_options(|options| options.read = true))
        .ok()?;
    let mut buf = vec![0; 64];
    let read = file.read_at(&mut buf, 0).ok()?;
    buf.truncate(read);
    Some(buf)
}

/// Make an empty file at `path`.
fn overwrite(filesystem: &dyn Filesystem, path: &str) {
    filesystem
        .open(
            path,
            &build_options(|options| (options.write, options.create) = (true, true)),
        )
        .expect("the file must be made");
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

/// A Memory storing every name in upper case while it reaches each by
/// any case, as a host that folds case does.
struct StoresUpperCase(Memory);

impl Filesystem for StoresUpperCase {
    holds_no_links!();
    never_repeats_identities!();

    fn open(&self, path: &str, options: &OpenOptions) -> Result<Box<dyn File>, FilesystemError> {
        self.0.open(path, options)
    }

    fn metadata(&self, path: &str) -> Result<Metadata, FilesystemError> {
        self.0.metadata(path)
    }

    fn stored_name(&self, name: &str) -> Result<String, FilesystemError> {
        Ok(name.to_uppercase())
    }

    opens_directories!(0);

    fn create_dir(&self, path: &str) -> Result<(), FilesystemError> {
        self.0.create_dir(path)
    }

    fn remove_dir(&self, path: &str) -> Result<(), FilesystemError> {
        self.0.remove_dir(path)
    }

    fn remove_file(&self, path: &str) -> Result<(), FilesystemError> {
        self.0.remove_file(path)
    }

    fn rename(
        &self,
        from: &str,
        _to_dir: &dyn Filesystem,
        to: &str,
        rename: Rename,
    ) -> Result<(), FilesystemError> {
        self.0.rename(from, &self.0, to, rename)
    }
}

// The Policy refuses only the stored spelling, so a Policy handed the
// spelling the guest asked for would let the read through.
// @behavior RT-120
#[test]
fn a_policy_judges_a_path_as_its_filesystem_stores_it() {
    let memory: Memory = [("file.txt", "granted")].into_iter().collect();
    let policy: Arc<dyn Policy> = Arc::new(Refuses("/data/FILE.TXT"));
    let mut guest = Guest::new(&[grant(
        Arc::new(StoresUpperCase(memory)),
        Access::Read,
        Some(policy),
    )]);
    assert_eq!(
        guest.call("read_file", FILE),
        ERRNO_ACCES,
        "path_open of `file.txt` beneath a Mount whose Policy refuses `/data/FILE.TXT`, over a \
         Filesystem storing that name as `FILE.TXT`, through preview1 must answer EACCES"
    );
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

// The first listing fills its buffer with `.` and `..` exactly, so it has
// not reached its end when the file is made.
// @behavior RT-107
#[test]
fn a_listing_goes_on_over_the_entries_it_began_with() {
    let memory: Arc<Memory> = Arc::new([("a.txt", "")].into_iter().collect());
    let mut guest = Guest::new(&[grant(memory.clone(), Access::ReadWrite, None)]);
    assert_eq!(
        guest.list(0, 51),
        [".", ".."],
        "the first listing must hold the dots"
    );
    memory
        .open(
            "b.txt",
            &build_options(|options| (options.write, options.create) = (true, true)),
        )
        .expect("the file must be made");
    assert_eq!(
        guest.list(2, 4096),
        ["a.txt"],
        "a listing going on through fd_readdir must read the entries it began with"
    );
    assert_eq!(
        guest.list(0, 4096),
        [".", "..", "a.txt", "b.txt"],
        "a listing from the start through fd_readdir must read the directory as it stands"
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

// A descriptor confines the guest to what lies beneath it, as cap-std
// confines wasmtime-wasi's, even where its Mount reaches further.
// @behavior RT-115
#[test]
fn a_path_above_the_directory_it_is_opened_beneath_is_refused() {
    let memory: Memory = [("file.txt", "granted"), ("dir/inner.txt", "beneath")]
        .into_iter()
        .collect();
    let mut guest = Guest::new(&[grant(Arc::new(memory), Access::Read, None)]);
    assert_eq!(
        guest.call("open_beneath_dir", ABOVE_ROOT),
        ERRNO_NOTCAPABLE,
        "a path climbing above a non-preopened directory descriptor through preview1 must answer \
         ENOTCAPABLE, as wasmtime-wasi does"
    );
}

/// What `stat_inner` answers through `dir` opened, beneath a Mount with
/// `policy`, before `change` runs on the Memory holding it.
fn stat_after(policy: Option<Arc<dyn Policy>>, change: impl FnOnce(&Memory)) -> i32 {
    let memory = file_and_directory();
    let mut guest = Guest::new(&[grant(memory.clone(), Access::ReadWrite, policy)]);
    assert_eq!(
        guest.call_bare("open_dir"),
        0,
        "dir must open as a directory"
    );
    change(&memory);
    guest.call_bare("stat_inner")
}

/// Move `dir` away, its `inner` with it, and make another `dir` holding
/// an `inner` of its own, so only where the descriptor reaches decides
/// whether the lookup finds one.
fn move_and_replace(memory: &Memory) {
    memory.rename("dir", memory, "moved", Rename::Any).unwrap();
    memory.create_dir("dir").unwrap();
    overwrite(memory, "dir/inner");
}

// A directory made at the path holds an `inner` of its own, so only the
// descriptor still standing for the removed one keeps it from reaching it.
// @behavior RT-117
#[test]
fn a_directory_made_in_a_removed_directorys_place_is_not_reached_through_it() {
    let answer = stat_after(None, |memory| {
        memory.remove_file("dir/inner").unwrap();
        memory.remove_dir("dir").unwrap();
        memory.create_dir("dir").unwrap();
        overwrite(memory, "dir/inner");
    });
    assert_eq!(
        answer, ERRNO_NOENT,
        "path_filestat_get through a directory descriptor beneath a Mount through preview1 must \
         answer ENOENT once the directory was removed and another made at its path"
    );
}

// @behavior RT-149
#[test]
fn a_directory_moved_while_opened_is_reached_through_its_descriptor() {
    assert_eq!(
        stat_after(None, move_and_replace),
        0,
        "path_filestat_get through a directory descriptor beneath a Mount without a Policy \
         through preview1 must reach the directory where it moved"
    );
}

// The Policy judges paths, so the descriptor reaches its directory from
// the Mount's root again and finds another one standing there.
// @behavior RT-150
#[test]
fn a_directory_moved_while_opened_beneath_a_policy_is_not_reached() {
    assert_eq!(
        stat_after(Some(Arc::new(AllowsAll)), move_and_replace),
        ERRNO_NOENT,
        "path_filestat_get through a directory descriptor beneath a Mount with a Policy through \
         preview1 must answer ENOENT once the directory moved away"
    );
}

/// A Memory naming `dir` by one inode number for as long as nothing holds
/// it, as a host hands a removed directory's number to the next one made,
/// and by a fresh number once a held `dir` was removed.
struct ReusesInodes {
    memory: Arc<Memory>,
    holds: Arc<AtomicUsize>,
    removed_while_held: AtomicUsize,
}

/// Counts as holding a directory until it drops.
struct Holding(Arc<AtomicUsize>);

impl Drop for Holding {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

impl Filesystem for ReusesInodes {
    holds_no_links!();
    matches_names_exactly!();

    fn open(&self, path: &str, options: &OpenOptions) -> Result<Box<dyn File>, FilesystemError> {
        self.memory.open(path, options)
    }

    fn metadata(&self, path: &str) -> Result<Metadata, FilesystemError> {
        let found = self.memory.metadata(path)?;
        if path != "dir" {
            return Ok(found);
        }
        let ino = 100 + self.removed_while_held.load(Ordering::SeqCst) as u64;
        Ok(found.with_identity(1, ino))
    }

    fn hold_directory(&self, path: &str) -> Result<Option<Hold>, FilesystemError> {
        if path != "dir" {
            return Ok(None);
        }
        self.holds.fetch_add(1, Ordering::SeqCst);
        Ok(Some(Hold::new(Holding(self.holds.clone()))))
    }

    opens_directories!(memory);

    fn create_dir(&self, path: &str) -> Result<(), FilesystemError> {
        self.memory.create_dir(path)
    }

    fn remove_dir(&self, path: &str) -> Result<(), FilesystemError> {
        self.memory.remove_dir(path)?;
        if self.holds.load(Ordering::SeqCst) > 0 {
            self.removed_while_held.fetch_add(1, Ordering::SeqCst);
        }
        Ok(())
    }

    fn remove_file(&self, path: &str) -> Result<(), FilesystemError> {
        self.memory.remove_file(path)
    }

    fn rename(
        &self,
        from: &str,
        _to_dir: &dyn Filesystem,
        to: &str,
        rename: Rename,
    ) -> Result<(), FilesystemError> {
        self.memory.rename(from, self.memory.as_ref(), to, rename)
    }
}

// The Filesystem gives the new `dir` the old one's number unless a Hold
// on it lives, so only the descriptor keeping its Hold refuses the call.
// @behavior RT-121
#[test]
fn a_directory_descriptor_holds_its_directory_while_open() {
    let memory = file_and_directory();
    let reuses = Arc::new(ReusesInodes {
        memory: memory.clone(),
        holds: Arc::new(AtomicUsize::new(0)),
        removed_while_held: AtomicUsize::new(0),
    });
    let mut guest = Guest::new(&[grant(reuses.clone(), Access::ReadWrite, None)]);
    assert_eq!(
        guest.call_bare("open_dir"),
        0,
        "dir must open as a directory"
    );
    reuses.remove_file("dir/inner").unwrap();
    reuses.remove_dir("dir").unwrap();
    reuses.create_dir("dir").unwrap();
    overwrite(memory.as_ref(), "dir/inner");
    assert_eq!(
        guest.call_bare("stat_inner"),
        ERRNO_NOENT,
        "path_filestat_get through a directory descriptor beneath a Mount through preview1, once \
         its directory is removed and made again on a Filesystem reusing inode numbers, must \
         answer ENOENT"
    );
}

// The open looks the directory up without following a link at its end, so
// the identity it remembers comes from the Overlay's lstat, and every later
// call compares it with the Overlay's stat.
#[test]
fn a_directory_two_layers_hold_is_reached_through_its_descriptor() {
    let upper: Arc<dyn Filesystem> = Arc::new([("dir/upper", "u")].into_iter().collect::<Memory>());
    let lower: Arc<dyn Filesystem> = file_and_directory();
    let overlay = Overlay::new()
        .layer(upper, Access::ReadWrite)
        .layer(lower, Access::Read);
    let mut guest = Guest::new(&[grant(Arc::new(overlay), Access::ReadWrite, None)]);
    assert_eq!(
        (guest.call_bare("open_dir"), guest.call_bare("stat_inner")),
        (0, 0),
        "path_filestat_get through a directory descriptor of an Overlay whose two Layers hold the \
         directory, beneath a Mount through preview1, must reach it"
    );
}

// @behavior RT-118
#[test]
fn a_directory_moved_back_to_its_path_is_reached_through_its_descriptor_again() {
    let answer = stat_after(None, |memory| {
        memory.rename("dir", memory, "moved", Rename::Any).unwrap();
        memory.rename("moved", memory, "dir", Rename::Any).unwrap();
    });
    assert_eq!(
        answer, 0,
        "path_filestat_get through a directory descriptor beneath a Mount through preview1 must \
         reach the directory once it moves away and back"
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
fn every_invocation_reaches_the_filesystem_its_mount_holds() {
    let memory = Arc::new(Memory::new());
    let grants = [grant(memory.clone(), Access::ReadWrite, None)];
    assert_eq!(
        Guest::new(&grants).call("write_file", FILE),
        0,
        "the write must succeed"
    );
    assert_eq!(
        Guest::new(&grants).call("read_file", FILE),
        0,
        "a file an earlier invocation wrote beneath a Mount over a Memory must be found by the \
         next invocation through preview1"
    );
    assert_eq!(
        contents(memory.as_ref(), "file.txt").as_deref(),
        Some(&b"hello"[..]),
        "the file the next invocation finds must hold what the earlier invocation wrote"
    );
}

/// A Memory holding `file.txt` that counts every change it is asked for,
/// so a test can tell the runtime refused before asking it.
struct ChangeCounter {
    memory: Memory,
    changes: AtomicUsize,
}

impl ChangeCounter {
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

impl Filesystem for ChangeCounter {
    holds_no_links!();
    matches_names_exactly!();
    never_repeats_identities!();

    fn open(&self, path: &str, options: &OpenOptions) -> Result<Box<dyn File>, FilesystemError> {
        self.memory.open(path, options)
    }

    fn metadata(&self, path: &str) -> Result<Metadata, FilesystemError> {
        self.memory.metadata(path)
    }

    opens_directories!(memory);

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

    fn rename(
        &self,
        from: &str,
        _to_dir: &dyn Filesystem,
        to: &str,
        rename: Rename,
    ) -> Result<(), FilesystemError> {
        self.change();
        self.memory.rename(from, &self.memory, to, rename)
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
    holds_no_links!();
    matches_names_exactly!();
    never_repeats_identities!();

    fn open(&self, path: &str, options: &OpenOptions) -> Result<Box<dyn File>, FilesystemError> {
        self.0.open(path, options)
    }

    fn metadata(&self, path: &str) -> Result<Metadata, FilesystemError> {
        self.0.metadata(path)
    }

    opens_directories!(0);

    fn create_dir(&self, path: &str) -> Result<(), FilesystemError> {
        self.0.create_dir(path)
    }

    fn remove_dir(&self, path: &str) -> Result<(), FilesystemError> {
        self.0.remove_dir(path)
    }

    fn remove_file(&self, path: &str) -> Result<(), FilesystemError> {
        self.0.remove_file(path)
    }

    fn rename(
        &self,
        from: &str,
        _to_dir: &dyn Filesystem,
        to: &str,
        rename: Rename,
    ) -> Result<(), FilesystemError> {
        self.0.remove_file(from)?;
        self.0.create_dir(from)?;
        drop(self.0.open(
            &format!("{from}/secret"),
            &build_options(|options| (options.write, options.create) = (true, true)),
        )?);
        self.0.rename(from, &self.0, to, rename)
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
    holds_no_links!();
    matches_names_exactly!();
    never_repeats_identities!();

    fn open(&self, path: &str, options: &OpenOptions) -> Result<Box<dyn File>, FilesystemError> {
        let writes = build_options(|options| options.write = true);
        Ok(Box::new(AppendedMeanwhileFile {
            file: self.0.open(path, options)?,
            other: std::sync::Mutex::new(self.0.open(path, &writes)?),
        }))
    }

    fn metadata(&self, path: &str) -> Result<Metadata, FilesystemError> {
        self.0.metadata(path)
    }

    opens_directories!(0);

    fn create_dir(&self, path: &str) -> Result<(), FilesystemError> {
        self.0.create_dir(path)
    }

    fn remove_dir(&self, path: &str) -> Result<(), FilesystemError> {
        self.0.remove_dir(path)
    }

    fn remove_file(&self, path: &str) -> Result<(), FilesystemError> {
        self.0.remove_file(path)
    }

    fn rename(
        &self,
        from: &str,
        _to_dir: &dyn Filesystem,
        to: &str,
        rename: Rename,
    ) -> Result<(), FilesystemError> {
        self.0.rename(from, &self.0, to, rename)
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

const FILETYPE_UNKNOWN: u8 = 0;
const FILETYPE_DIRECTORY: u8 = 3;
const FILETYPE_REGULAR_FILE: u8 = 4;
const FILETYPE_SYMBOLIC_LINK: u8 = 7;

/// A host directory of its own holding `file.txt` as a link to `target`,
/// made by `make_target`, and a guest handed it as a read-only Directory
/// Mount.
#[cfg(unix)]
fn linked(name: &str, make_target: fn(&std::path::Path)) -> (std::path::PathBuf, Guest) {
    linked_under(name, make_target, Access::Read, None)
}

/// As `linked`, the Mount taking `access` and `policy`.
#[cfg(unix)]
fn linked_under(
    name: &str,
    make_target: fn(&std::path::Path),
    access: Access,
    policy: Option<Arc<dyn Policy>>,
) -> (std::path::PathBuf, Guest) {
    let host = std::env::temp_dir().join(format!("kobako-granted-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&host);
    std::fs::create_dir_all(&host).expect("the host directory must be created");
    make_target(&host.join("target"));
    std::os::unix::fs::symlink("target", host.join("file.txt")).expect("the link must be made");
    let directory = Directory::new(&host).expect("the host directory must open");
    let guest = Guest::new(&[grant(Arc::new(directory), access, policy)]);
    (host, guest)
}

/// What each stat of `path` with the given lookupflags answers, and the
/// filetype it wrote, beneath a Directory Mount whose host directory
/// holds `file.txt` as a link to `target`, made by `make_target`.
#[cfg(unix)]
fn stat_link(
    name: &str,
    make_target: fn(&std::path::Path),
    stats: &[((i32, i32), i32)],
) -> Vec<(i32, u8)> {
    let (host, mut guest) = linked(name, make_target);
    let answers = stats
        .iter()
        .map(|&(path, flags)| {
            let errno = guest
                .instance
                .get_typed_func::<(i32, i32, i32), i32>(&mut guest.store, "stat_flagged")
                .expect("probe export must resolve")
                .call(&mut guest.store, (path.0, path.1, flags))
                .expect("probe call must not trap");
            let memory = guest
                .instance
                .get_memory(&mut guest.store, "memory")
                .expect("probe exports its memory");
            (errno, memory.data(&guest.store)[384 + 16])
        })
        .collect();
    let _ = std::fs::remove_dir_all(&host);
    answers
}

// A FIFO with no writer would hold an open that waits for one, so only
// an open refused before it waits returns at all.
// @behavior RT-127
#[cfg(unix)]
#[test]
fn a_guest_sees_a_fifo_as_of_an_unknown_type_and_cannot_open_it() {
    let host = std::env::temp_dir().join(format!("kobako-granted-fifo-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&host);
    std::fs::create_dir_all(&host).expect("the host directory must be created");
    let made = std::process::Command::new("mkfifo")
        .arg(host.join("file.txt"))
        .status()
        .expect("mkfifo must run");
    assert!(made.success(), "the FIFO must be made");
    let directory = Directory::new(&host).expect("the host directory must open");
    let mut guest = Guest::new(&[grant(Arc::new(directory), Access::Read, None)]);
    let stat = guest
        .instance
        .get_typed_func::<(i32, i32, i32), i32>(&mut guest.store, "stat_flagged")
        .expect("probe export must resolve")
        .call(&mut guest.store, (FILE.0, FILE.1, 0))
        .expect("probe call must not trap");
    let filestat = guest
        .instance
        .get_memory(&mut guest.store, "memory")
        .expect("probe exports its memory")
        .data(&guest.store)[384..384 + 32]
        .to_vec();
    let nlink = u64::from_le_bytes(filestat[24..32].try_into().expect("nlink is 8 bytes"));
    let opened = guest.call("read_file", FILE);
    let _ = std::fs::remove_dir_all(&host);
    assert_eq!(
        (stat, filestat[16], nlink, opened),
        (0, FILETYPE_UNKNOWN, 1, ERRNO_ACCES),
        "path_filestat_get of a FIFO beneath a Directory Mount through preview1 must report an \
         unknown filetype named once, and path_open of it must answer EACCES"
    );
}

// The link points at a regular file, so only a stat that follows none
// can answer a link.
// @behavior RT-097
#[cfg(unix)]
#[test]
fn a_stat_following_no_link_reports_the_link_itself() {
    let answers = stat_link(
        "lstat",
        |target| std::fs::write(target, "granted").expect("the host file must be written"),
        &[(FILE, 1), (FILE, 0)],
    );
    assert_eq!(
        answers,
        [(0, FILETYPE_REGULAR_FILE), (0, FILETYPE_SYMBOLIC_LINK)],
        "path_filestat_get of a host link beneath a Directory Mount through preview1 must report \
         the file it points to when following links, and a symbolic link when following none"
    );
}

// The stat follows no link, so only the trailing `/` can make it reach
// the directory the link points at, as Linux's `lstat` does.
// @behavior RT-098
#[cfg(unix)]
#[test]
fn a_stat_of_a_link_named_with_a_trailing_slash_reports_its_directory() {
    let answers = stat_link(
        "lstat-slash",
        |target| std::fs::create_dir(target).expect("the host directory must be created"),
        &[(FILE_AS_DIRECTORY, 0), (FILE, 0)],
    );
    assert_eq!(
        answers,
        [(0, FILETYPE_DIRECTORY), (0, FILETYPE_SYMBOLIC_LINK)],
        "path_filestat_get following no link of a host link to a directory, named with a \
         trailing slash, beneath a Directory Mount through preview1 must report the directory, \
         and without the slash a symbolic link"
    );
}

/// What `path_unlink_file` and `path_rename` answer for the link
/// `file.txt/` to a directory beneath a read-write Directory Mount taking
/// `policy`, and whether the link stays; `name` keeps its host directory
/// its own.
#[cfg(unix)]
fn change_link_named_as_directory(name: &str, policy: Option<Arc<dyn Policy>>) -> ([i32; 2], bool) {
    let (host, mut guest) = linked_under(name, directory_with_key, Access::ReadWrite, policy);
    let answers = [
        guest.call("unlink_path", FILE_AS_DIRECTORY),
        guest.call_pair("rename_paths", FILE_AS_DIRECTORY, MOVED),
    ];
    let kept = host.join("file.txt").symlink_metadata().is_ok();
    let _ = std::fs::remove_dir_all(&host);
    (answers, kept)
}

// Linux's `unlink` and `rename` both act on the link itself, which is no
// directory, so a Policy, refusing only a link a call follows, changes
// neither answer.
// @behavior RT-123
#[cfg(unix)]
#[test]
fn a_link_named_with_a_trailing_slash_is_neither_removed_nor_moved() {
    assert_eq!(
        [
            change_link_named_as_directory("link-slash", None),
            change_link_named_as_directory("policy-link-slash", Some(Arc::new(AllowsAll))),
        ],
        [([ERRNO_NOTDIR, ERRNO_NOTDIR], true); 2],
        "path_unlink_file and path_rename of a host link to a directory named with a trailing \
         slash, beneath a read-write Directory Mount with or without a Policy through preview1, \
         must answer ENOTDIR as Linux does and leave the link"
    );
}

// The link points at nothing, so only an unlink that follows no link
// finds it there to answer ENOTDIR rather than ENOENT.
// @behavior RT-134
#[cfg(unix)]
#[test]
fn an_unlink_with_a_trailing_slash_answers_for_the_name_itself() {
    let host = std::env::temp_dir().join(format!(
        "kobako-granted-unlink-slash-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&host);
    std::fs::create_dir_all(host.join("dir")).expect("the host directory must be created");
    std::fs::write(host.join("file.txt"), "granted").expect("the host file must be written");
    std::os::unix::fs::symlink("nowhere", host.join("gone")).expect("the link must be made");
    let directory = Directory::new(&host).expect("the host directory must open");
    let mut guest = Guest::new(&[grant(Arc::new(directory), Access::ReadWrite, None)]);
    let answers = [
        DIR_AS_DIRECTORY,
        FILE_AS_DIRECTORY,
        GONE_AS_DIRECTORY,
        NEW_AS_DIRECTORY,
    ]
    .map(|path| guest.call("unlink_path", path));
    let kept = ["dir", "file.txt", "gone"].map(|name| host.join(name).symlink_metadata().is_ok());
    let _ = std::fs::remove_dir_all(&host);
    assert_eq!(
        (answers, kept),
        (
            [ERRNO_ISDIR, ERRNO_NOTDIR, ERRNO_NOTDIR, ERRNO_NOENT],
            [true; 3]
        ),
        "path_unlink_file of a directory, a file, a link to nothing, and a missing name, each \
         named with a trailing slash beneath a read-write Directory Mount through preview1, must \
         answer EISDIR, ENOTDIR, ENOTDIR, and ENOENT as Linux does and remove nothing"
    );
}

// @behavior RT-099
#[test]
fn an_allocation_for_a_file_beneath_a_mount_is_unsupported() {
    let memory: Arc<Memory> = Arc::new([("file.txt", "granted")].into_iter().collect());
    let answer =
        Guest::new(&[grant(memory.clone(), Access::ReadWrite, None)]).call("allocate_file", FILE);
    assert_eq!(
        (answer, contents(memory.as_ref(), "file.txt")),
        (ERRNO_NOTSUP, Some(b"granted".to_vec())),
        "fd_allocate of a file beneath a read-write Mount through preview1 must answer ENOTSUP \
         as wasmtime-wasi does, leaving the file as it was"
    );
}

// The probe's opens follow no link, so only the trailing `/` can make one
// reach the directory the link points at, as Linux's `open` does.
// @behavior RT-100
#[cfg(unix)]
#[test]
fn an_open_following_no_link_refuses_a_link_to_a_directory() {
    let (host, mut guest) = linked("open-nofollow", |target| {
        std::fs::create_dir(target).expect("the host directory must be created")
    });
    let answers = [
        guest.call_flagged(FILE, 0, 0),
        guest.call_flagged(FILE_AS_DIRECTORY, 0, 0),
    ];
    let _ = std::fs::remove_dir_all(&host);
    assert_eq!(
        answers,
        [ERRNO_LOOP, 0],
        "path_open following no link of a host link to a directory beneath a Directory Mount \
         through preview1 must answer ELOOP, and with a trailing slash open the directory"
    );
}

/// A host directory linked in as `file.txt`, holding `key`.
#[cfg(unix)]
fn directory_with_key(target: &std::path::Path) {
    std::fs::create_dir(target).expect("the host directory must be created");
    std::fs::write(target.join("key"), "secret").expect("the host file must be written");
}

// The Policy allows everything, so only the link on the way can refuse
// these; the same calls beneath a Mount without a Policy go through.
// @behavior RT-101
#[cfg(unix)]
#[test]
fn a_path_through_a_link_beneath_a_mount_with_a_policy_answers_eloop() {
    let policy: Arc<dyn Policy> = Arc::new(AllowsAll);
    let (host, mut guest) = linked_under(
        "policy-link",
        directory_with_key,
        Access::ReadWrite,
        Some(policy),
    );
    let refused = [
        guest.call("read_file", THROUGH_FILE),
        guest.call("unlink_path", THROUGH_FILE),
        guest.call("open_following", FILE),
    ];
    let kept = host.join("target/key").exists();
    let _ = std::fs::remove_dir_all(&host);
    let (host, mut guest) = linked_under("plain-link", directory_with_key, Access::Read, None);
    let followed = [
        guest.call("read_file", THROUGH_FILE),
        guest.call("open_following", FILE),
    ];
    let _ = std::fs::remove_dir_all(&host);
    assert_eq!(
        (refused, kept, followed),
        ([ERRNO_LOOP; 3], true, [0, 0]),
        "path_open and path_unlink_file through a host link, or path_open following one at the \
         end, beneath a Mount with a Policy through preview1 must answer ELOOP and change \
         nothing, while a Mount without a Policy follows the link"
    );
}

// cap-std refuses to follow a link out of the host directory, and the
// host refuses a loop, so each open is refused whichever answers first;
// only ELOOP says the link is why. An unlink acts on the link itself,
// which no Policy-free walk past it is needed for.
// @behavior RT-125
#[cfg(unix)]
#[test]
fn a_link_out_of_a_directory_or_into_a_loop_answers_eloop_beneath_a_policy() {
    let outside = std::env::temp_dir().join(format!("kobako-outside-{}", std::process::id()));
    directory_with_key(&outside);
    let targets = [
        outside.to_string_lossy().into_owned(),
        format!("../{}", outside.file_name().unwrap().to_string_lossy()),
        "file.txt".to_string(),
    ];
    let answers: Vec<_> = targets
        .iter()
        .enumerate()
        .map(|(index, target)| {
            let host = std::env::temp_dir().join(format!(
                "kobako-granted-escaping-{index}-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&host);
            std::fs::create_dir_all(&host).expect("the host directory must be created");
            std::os::unix::fs::symlink(target, host.join("file.txt"))
                .expect("the link must be made");
            let directory = Directory::new(&host).expect("the host directory must open");
            let policy: Arc<dyn Policy> = Arc::new(AllowsAll);
            let mut guest =
                Guest::new(&[grant(Arc::new(directory), Access::ReadWrite, Some(policy))]);
            let answers = [
                guest.call("read_file", THROUGH_FILE),
                guest.call("unlink_path", FILE),
            ];
            let removed = host.join("file.txt").symlink_metadata().is_err();
            let _ = std::fs::remove_dir_all(&host);
            (answers, removed)
        })
        .collect();
    let kept = outside.join("key").exists();
    let _ = std::fs::remove_dir_all(&outside);
    assert_eq!(
        (answers, kept),
        (vec![([ERRNO_LOOP, 0], true); 3], true),
        "path_open through a host link out of a Directory, by an absolute or a relative target, \
         or into a loop, beneath a Mount with a Policy through preview1 must answer ELOOP, and \
         path_unlink_file of each link must remove it"
    );
}

// The Filesystem fails only the lookup of `dir`, which the open needs
// no answer from but the walk for links on its way does.
// @behavior RT-126
#[test]
fn a_link_check_that_fails_fails_the_call_beneath_a_policy() {
    let failing = Arc::new(FailsLookingUpDir(
        [("dir/file.txt", "granted")].into_iter().collect(),
    ));
    let policy: Arc<dyn Policy> = Arc::new(AllowsAll);
    let mut guest = Guest::new(&[grant(failing, Access::Read, Some(policy))]);
    assert_eq!(
        guest.call("read_file", DIR_FILE),
        ERRNO_IO,
        "path_open of a file beneath a directory the Filesystem fails to look up, beneath a \
         Mount with a Policy through preview1, must answer that failure"
    );
}

/// A Memory failing every lookup of `dir` itself, as a host directory
/// that cannot be read could.
struct FailsLookingUpDir(Memory);

impl Filesystem for FailsLookingUpDir {
    matches_names_exactly!();
    never_repeats_identities!();

    fn open(&self, path: &str, options: &OpenOptions) -> Result<Box<dyn File>, FilesystemError> {
        self.0.open(path, options)
    }

    fn metadata(&self, path: &str) -> Result<Metadata, FilesystemError> {
        self.0.metadata(path)
    }

    fn symlink_metadata(&self, path: &str) -> Result<Metadata, FilesystemError> {
        match path {
            "dir" => Err(FilesystemError::Other("unreadable".into())),
            path => self.0.symlink_metadata(path),
        }
    }

    fn canonicalize_beneath(&self, base: &str, path: &str) -> Result<String, FilesystemError> {
        self.0.canonicalize_beneath(base, path)
    }

    fn for_policy(&self) -> Option<Arc<dyn Filesystem>> {
        None
    }

    opens_directories!(0);

    fn create_dir(&self, path: &str) -> Result<(), FilesystemError> {
        self.0.create_dir(path)
    }

    fn remove_dir(&self, path: &str) -> Result<(), FilesystemError> {
        self.0.remove_dir(path)
    }

    fn remove_file(&self, path: &str) -> Result<(), FilesystemError> {
        self.0.remove_file(path)
    }

    fn rename(
        &self,
        from: &str,
        _to_dir: &dyn Filesystem,
        to: &str,
        rename: Rename,
    ) -> Result<(), FilesystemError> {
        self.0.rename(from, &self.0, to, rename)
    }
}

// The Mount has no Policy and follows links, so only the step out of
// the link can refuse these.
// @behavior RT-119
#[cfg(unix)]
#[test]
fn a_step_out_of_a_link_answers_eloop() {
    assert_eq!(
        stat_link(
            "dot-link",
            directory_with_key,
            &[(LINK_DOT, 1), (OUT_OF_LINK, 0)]
        )
        .into_iter()
        .map(|(errno, _)| errno)
        .collect::<Vec<_>>(),
        [ERRNO_LOOP; 2],
        "path_filestat_get of `file.txt/.` and `file.txt/../file.txt`, where `file.txt` is a \
         host link to a directory, beneath a Directory Mount through preview1 must answer ELOOP"
    );
}

fn file_and_directory() -> Arc<Memory> {
    Arc::new(
        [("file.txt", "granted"), ("dir/inner", "x")]
            .into_iter()
            .collect(),
    )
}

// A `.` after a name resolves through it, so Linux and macOS both refuse
// `missing/.` at the missing name rather than make it.
// @behavior RT-109
#[test]
fn a_path_ending_in_a_dot_makes_nothing_where_no_directory_stands() {
    let memory = file_and_directory();
    let mut guest = Guest::new(&[grant(memory.clone(), Access::ReadWrite, None)]);
    let answers = [
        guest.call("make_dir", MISSING_DOT),
        guest.call_flagged(MISSING_DOT, OFLAGS_CREAT, 0),
    ];
    assert_eq!(
        (answers, memory.metadata("missing").is_err()),
        ([ERRNO_NOENT, ERRNO_NOENT], true),
        "path_create_directory and path_open with O_CREAT of missing/. beneath a Mount through \
         preview1 must answer ENOENT, as Linux does, and make nothing named missing"
    );
}

// Resolving `dir/.` reaches `dir` itself, so only refusing the path as
// written keeps either call from taking `dir`.
// @behavior RT-102
#[test]
fn a_path_ending_in_a_dot_is_neither_removed_nor_moved() {
    let memory = file_and_directory();
    let mut guest = Guest::new(&[grant(memory.clone(), Access::ReadWrite, None)]);
    let answers = [
        guest.call("remove_path", DIR_DOT),
        guest.call("remove_path", DIR_DOT_DOT),
        guest.call_pair("rename_paths", DIR_DOT, MOVED),
        guest.call_pair("rename_paths", DIR_DOT_DOT, MOVED),
        guest.call("remove_path", OUT_OF_FILE),
        guest.call_pair("rename_paths", OUT_OF_FILE, MOVED),
    ];
    assert_eq!(
        (answers, memory.metadata("dir/inner").is_ok()),
        (
            [
                ERRNO_INVAL,
                ERRNO_INVAL,
                ERRNO_BUSY,
                ERRNO_BUSY,
                ERRNO_NOTDIR,
                ERRNO_NOTDIR
            ],
            true
        ),
        "path_remove_directory of a path ending in . or .., and path_rename of one, beneath a \
         Mount through preview1 must answer EINVAL and EBUSY as wasmtime-wasi does, or ENOTDIR \
         where a file stands before the dot, and leave the directory in place"
    );
}

// The Mount only reads, so only judging a create of what already stands
// as no change lets the open through.
// @behavior RT-103
#[test]
fn a_create_of_what_already_stands_changes_nothing() {
    let mut guest = Guest::new(&[grant(file_and_directory(), Access::Read, None)]);
    let answers = [
        guest.call_flagged(FILE, OFLAGS_CREAT, 0),
        guest.call_flagged(DIR, OFLAGS_CREAT, 0),
    ];
    assert_eq!(
        answers,
        [0, ERRNO_ISDIR],
        "path_open with O_CREAT and no write of an existing file beneath a read-only Mount \
         through preview1 must open it, and of an existing directory answer EISDIR, as Linux does"
    );
}

// The host file only reads, so a host open asking to create, and so to
// write, would be refused where the guest asked only to read.
// @behavior RT-103
#[cfg(unix)]
#[test]
fn a_create_of_a_host_file_that_only_reads_opens_it() {
    use std::os::unix::fs::PermissionsExt;
    let host = std::env::temp_dir().join(format!("kobako-granted-creat-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&host);
    std::fs::create_dir_all(&host).expect("the host directory must be created");
    std::fs::write(host.join("file.txt"), "granted").expect("the host file must be written");
    std::fs::set_permissions(
        host.join("file.txt"),
        std::fs::Permissions::from_mode(0o444),
    )
    .expect("the host file must become read-only");
    let directory = Directory::new(&host).expect("the host directory must open");
    let answer = Guest::new(&[grant(Arc::new(directory), Access::ReadWrite, None)]).call_flagged(
        FILE,
        OFLAGS_CREAT,
        0,
    );
    let _ = std::fs::remove_dir_all(&host);
    assert_eq!(
        answer, 0,
        "path_open with O_CREAT and no write of a read-only host file beneath a Directory Mount \
         through preview1 must open it"
    );
}

// The second seek would land past what a signed offset holds, so only
// the cap keeps the position from reaching it.
// @behavior RT-104
#[test]
fn a_seek_past_the_largest_offset_overflows() {
    let answer = Guest::new(&[grant(file_and_directory(), Access::Read, None)])
        .call_bare("seek_past_the_largest_offset");
    assert_eq!(
        answer, ERRNO_OVERFLOW,
        "fd_seek past the largest signed 64-bit offset of a file beneath a Mount through \
         preview1 must answer EOVERFLOW, as Linux does"
    );
}

// A Memory clamps a read past its end and tries to grow for a write or a
// size, so only the check before the Filesystem answers each as Linux does.
// @behavior RT-116
#[test]
fn an_offset_past_the_largest_signed_offset_is_invalid() {
    let mut guest = Guest::new(&[grant(file_and_directory(), Access::ReadWrite, None)]);
    assert_eq!(
        (
            guest.call_at("read_at", 1 << 63),
            guest.call_at("write_at", 1 << 63),
            guest.call_at("size_at", 1 << 63)
        ),
        (ERRNO_INVAL, ERRNO_INVAL, ERRNO_INVAL),
        "fd_pread, fd_pwrite, and fd_filestat_set_size past the largest signed 64-bit offset of a \
         Memory file beneath a Mount through preview1 must answer EINVAL, as Linux does"
    );
}

// The file is far shorter than either offset, so a read reaching the
// Filesystem answers its end; only the refusal before it answers EINVAL.
// @behavior RT-136
#[test]
fn a_read_whose_end_passes_the_largest_signed_offset_is_invalid() {
    let mut guest = Guest::new(&[grant(file_and_directory(), Access::ReadWrite, None)]);
    assert_eq!(
        (
            guest.call_at("read_at", i64::MAX as u64 - 10),
            guest.call_bare("read_near_the_largest_offset"),
            guest.call_at("read_at", i64::MAX as u64 - 64),
        ),
        (ERRNO_INVAL, ERRNO_INVAL, 0),
        "fd_pread and fd_read of 64 bytes from ten before the largest signed 64-bit offset of a \
         file beneath a Mount through preview1 must answer EINVAL as Linux does, while a read \
         ending at it answers the file's end"
    );
}

// Each file is opened only to read, so only its Mount's access can tell the
// two answers apart.
// @behavior RT-135
#[test]
fn a_wait_to_write_a_file_answers_by_what_its_mount_permits() {
    let wait = |access| {
        Guest::new(&[grant(file_and_directory(), access, None)])
            .call_at("poll_write_with", RIGHTS_FD_READ)
    };
    assert_eq!(
        (wait(Access::ReadWrite), wait(Access::Read)),
        (0, ERRNO_PERM),
        "poll_oneoff waiting to write a file opened only to read through preview1 must be ready \
         beneath a read-write Mount and answer EPERM beneath a read-only one, as wasmtime-wasi \
         answers by its preopen"
    );
}

// The same file opened with the same rights, so only its Mount's access
// tells the two refusals apart.
// @behavior RT-137
#[test]
fn sizing_a_file_opened_only_to_read_answers_by_its_mount_then_as_linux_does() {
    let size = |access, rights| {
        Guest::new(&[grant(file_and_directory(), access, None)]).call_at("size_with", rights)
    };
    assert_eq!(
        (
            size(Access::Read, RIGHTS_FD_READ),
            size(Access::ReadWrite, RIGHTS_FD_READ),
            size(Access::ReadWrite, RIGHTS_FD_WRITE),
        ),
        (ERRNO_PERM, ERRNO_INVAL, 0),
        "fd_filestat_set_size on a file opened only to read through preview1 must answer EPERM \
         beneath a read-only Mount, as wasmtime-wasi does, and EINVAL beneath a read-write one, \
         as Linux's ftruncate does, while a file opened to write is sized"
    );
}

/// A Policy allowing every change while keeping each one it is asked.
#[derive(Default)]
struct RecordsAsks(std::sync::Mutex<Vec<(String, Access)>>);

impl Policy for RecordsAsks {
    fn allows(&self, path: &str, access: Access) -> bool {
        self.0.lock().unwrap().push((path.to_owned(), access));
        true
    }
}

// The Mount is made read-only, as a Ruby mount with a block is by default,
// so only the Policy standing in its access's place lets either through.
// Each Policy is asked only by the open before the call, so a wait or a size
// asking it as well adds an ask the literal lists below do not hold.
// @behavior RT-139
#[test]
fn a_mount_whose_policy_decides_may_be_written() {
    let call = |name, rights| {
        let policy = Arc::new(RecordsAsks::default());
        let errno = Guest::new(&[grant(
            file_and_directory(),
            Access::Read,
            Some(policy.clone() as Arc<dyn Policy>),
        )])
        .call_at(name, rights);
        let asked = policy.0.lock().unwrap().clone();
        (errno, asked)
    };
    assert_eq!(
        (
            call("poll_write_with", RIGHTS_FD_READ),
            call("size_with", RIGHTS_FD_WRITE),
        ),
        (
            (0, vec![("/data/file.txt".to_owned(), Access::Read)]),
            (0, vec![("/data/file.txt".to_owned(), Access::ReadWrite)]),
        ),
        "through preview1 beneath a read-only Mount whose Policy allows every change, \
         poll_oneoff waiting to write a file opened only to read must be ready, and \
         fd_filestat_set_size on a file opened to write must size it, the Policy asked \
         only by each open"
    );
}

// Each open asks without the right to write, so only its CREAT, where
// nothing stands yet, makes it a change.
// @behavior RT-146
#[test]
fn a_policy_is_asked_once_for_an_open_that_may_create_as_what_it_does() {
    let open = |path| {
        let policy = Arc::new(RecordsAsks::default());
        let errno = Guest::new(&[grant(
            file_and_directory(),
            Access::Read,
            Some(policy.clone() as Arc<dyn Policy>),
        )])
        .call_flagged(path, OFLAGS_CREAT, 0);
        let asked = policy.0.lock().unwrap().clone();
        (errno, asked)
    };
    assert_eq!(
        (open(NEW), open(FILE)),
        (
            (0, vec![("/data/new.txt".to_owned(), Access::ReadWrite)]),
            (0, vec![("/data/file.txt".to_owned(), Access::Read)]),
        ),
        "path_open with O_CREAT and no write of a missing path and of an existing file beneath \
         a Mount whose Policy allows every change through preview1 must ask the Policy once for \
         each, as a change where it creates and as a read where the file stands"
    );
}

// The open asks to write a path nothing stands at, so only the lookup
// before the Mount judges it answers ENOENT, as Linux does on a read-only
// mount, rather than the EROFS the same open of a file answers.
// @behavior RT-147
#[test]
fn an_open_of_a_missing_path_that_makes_nothing_answers_enoent() {
    let read_only =
        Guest::new(&[grant(file_and_directory(), Access::Read, None)]).call("append_file", NEW);
    let policy = Arc::new(RecordsAsks::default());
    let policed = Guest::new(&[grant(
        file_and_directory(),
        Access::Read,
        Some(policy.clone() as Arc<dyn Policy>),
    )])
    .call("append_file", NEW);
    let asked = policy.0.lock().unwrap().clone();
    assert_eq!(
        (read_only, policed, asked),
        (ERRNO_NOENT, ERRNO_NOENT, Vec::new()),
        "path_open to write a missing path without O_CREAT beneath a read-only Mount and beneath \
         a Mount with a Policy through preview1 must answer ENOENT and ask the Policy nothing"
    );
}

// The Mount is read-only, so an open judged before what stands at its
// path is looked up would answer EROFS for both.
// @behavior RT-148
#[test]
fn what_stands_at_a_path_answers_before_a_read_only_mount_refuses() {
    let mut guest = Guest::new(&[grant(file_and_directory(), Access::Read, None)]);
    assert_eq!(
        (
            guest.call_flagged(FILE, OFLAGS_CREAT | OFLAGS_EXCL, 0),
            guest.call("append_file", DIR),
        ),
        (ERRNO_EXIST, ERRNO_ISDIR),
        "path_open with O_CREAT | O_EXCL of a file, and to write a directory, beneath a read-only \
         Mount through preview1 must answer EEXIST and EISDIR, as Linux does"
    );
}

const NEW: (i32, i32) = (840, 7);
const RIGHTS_FD_READ: u64 = 1 << 1;
const ERRNO_PERM: i32 = 63;

// A Memory would refuse the bytes for want of space, so only the refusal
// before the Filesystem is asked answers EFBIG.
// @behavior RT-133
#[test]
fn a_write_at_the_largest_signed_offset_is_too_large() {
    let memory = file_and_directory();
    let answer = Guest::new(&[grant(memory.clone(), Access::ReadWrite, None)])
        .call_bare("write_at_the_largest_offset");
    assert_eq!(
        (answer, contents(memory.as_ref(), "file.txt")),
        (ERRNO_FBIG, Some(b"granted".to_vec())),
        "fd_write at the largest signed 64-bit offset of a Memory file beneath a Mount through \
         preview1 must answer EFBIG, as Linux does, and leave the file as it was"
    );
}

// @behavior RT-130
#[test]
fn an_advice_past_the_largest_signed_offset_is_invalid() {
    let mut guest = Guest::new(&[grant(file_and_directory(), Access::ReadWrite, None)]);
    assert_eq!(
        (
            guest.call_at("advise_from", 1 << 63),
            guest.call_at("advise_for", 1 << 63),
            guest.call_at("advise_for", 0)
        ),
        (ERRNO_INVAL, ERRNO_INVAL, 0),
        "fd_advise from or for past the largest signed 64-bit offset of a Memory file beneath a \
         Mount through preview1 must answer EINVAL, as an advice within it succeeds"
    );
}

// The open asks to read only, so nothing else the open could refuse
// stands in for the refusal of not blocking.
// @behavior RT-129
#[test]
fn a_descriptor_that_would_not_block_is_unsupported() {
    let mut guest = Guest::new(&[grant(file_and_directory(), Access::ReadWrite, None)]);
    assert_eq!(
        (
            guest.call_flagged(FILE, 0, FDFLAGS_NONBLOCK),
            guest.call_bare("set_nonblocking"),
            guest.call_flagged(FILE, 0, 0)
        ),
        (ERRNO_NOTSUP, ERRNO_NOTSUP, 0),
        "path_open asking not to block and fd_fdstat_set_flags setting an open file not to \
         block, beneath a Mount through preview1, must answer ENOTSUP"
    );
}

// @behavior RT-128
#[test]
fn a_directory_a_guest_opens_carries_no_right_to_write() {
    let mut guest = Guest::new(&[grant(file_and_directory(), Access::ReadWrite, None)]);
    let errno = guest.call_bare("dir_fdstat");
    let memory = guest
        .instance
        .get_memory(&mut guest.store, "memory")
        .expect("probe exports its memory");
    let rights = u64::from_le_bytes(
        memory.data(&guest.store)[392..400]
            .try_into()
            .expect("the rights are eight bytes"),
    );
    assert_eq!(
        (errno, rights & RIGHTS_FD_WRITE, rights & RIGHTS_FD_READDIR),
        (0, 0, RIGHTS_FD_READDIR),
        "fd_fdstat_get of a directory opened through path_open beneath a read-write Mount \
         through preview1 must report its rights without fd_write, as wasmtime-wasi does"
    );
}

// @behavior RT-105
#[test]
fn a_file_moved_beneath_itself_is_no_directory() {
    let memory = file_and_directory();
    let answer = Guest::new(&[grant(memory.clone(), Access::ReadWrite, None)]).call_pair(
        "rename_paths",
        FILE,
        BENEATH_FILE,
    );
    assert_eq!(
        (answer, contents(memory.as_ref(), "file.txt")),
        (ERRNO_NOTDIR, Some(b"granted".to_vec())),
        "path_rename of a file to a path beneath it, beneath a Mount through preview1, must \
         answer ENOTDIR as Linux does and leave the file in place"
    );
}

// The link points at nothing, so only the link itself, not a file it
// would follow to, can be what each open finds there.
// @behavior RT-106
#[cfg(unix)]
#[test]
fn an_open_following_no_link_finds_a_dangling_link_as_itself() {
    let (host, mut guest) = linked_under("dangling", |_| {}, Access::ReadWrite, None);
    let answers = [
        guest.call_flagged(FILE, 0, 0),
        guest.call_flagged(FILE, OFLAGS_CREAT | OFLAGS_EXCL, 0),
    ];
    let made = host.join("target").exists();
    let _ = std::fs::remove_dir_all(&host);
    assert_eq!(
        (answers, made),
        ([ERRNO_LOOP, ERRNO_EXIST], false),
        "path_open following no link of a dangling host link beneath a Directory Mount through \
         preview1 must answer ELOOP, and with O_CREAT and O_EXCL EEXIST, as Linux does, making \
         nothing where it points"
    );
}

/// What each root-changing export answers, and how many changes the
/// Filesystem was asked for along the way.
fn change_root(exports: &[&str]) -> (Vec<i32>, usize) {
    let counted = ChangeCounter::new();
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

// The Memory would hold `file.txt` and any other name, so only the NUL
// refuses the open.
// @behavior RT-122
#[test]
fn a_path_holding_a_nul_is_invalid() {
    assert_eq!(
        read_beneath(HOLDING_NUL),
        ERRNO_INVAL,
        "path_open of `file.txt\\0` beneath a Mount through preview1 must answer EINVAL"
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
            memory.entries().map(|entries| entries.len())
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
const FDFLAGS_NONBLOCK: i32 = 1 << 2;
const RIGHTS_FD_WRITE: u64 = 1 << 6;
const RIGHTS_FD_READDIR: u64 = 1 << 14;

/// What opening `path` with `oflags` and `fdflags` answers beneath a
/// read-write Memory holding `file.txt`, and the paths it holds after.
fn open_flagged(path: (i32, i32), oflags: i32, fdflags: i32) -> (i32, Vec<String>) {
    let memory: Arc<Memory> = Arc::new([("file.txt", "granted")].into_iter().collect());
    let errno = Guest::new(&[grant(memory.clone(), Access::ReadWrite, None)])
        .call_flagged(path, oflags, fdflags);
    let held = memory
        .entries()
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

const UP: (i32, i32) = (1070, 2);
const THROUGH_UPDIR: (i32, i32) = (1080, 7);
const IN_LINKED: (i32, i32) = (1090, 4);
const TWISTY: (i32, i32) = (1100, 6);
const DANGLE: (i32, i32) = (1110, 6);
const DIR_UP: (i32, i32) = (1120, 6);
const NOWHERE: (i32, i32) = (1130, 7);

/// A host directory whose `dir` holds links staying within it (`in`,
/// `twisty`), links leading out of it (`up`, `updir`), and links pointing
/// at nothing out of it (`dangle`) and within it (`nowhere`).
#[cfg(unix)]
struct LinkedBeneath(std::path::PathBuf);

#[cfg(unix)]
impl LinkedBeneath {
    fn new(name: &str) -> Self {
        let host =
            std::env::temp_dir().join(format!("kobako-beneath-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&host);
        std::fs::create_dir_all(host.join("dir/sub")).expect("the host directory must be created");
        std::fs::create_dir_all(host.join("other")).expect("the host directory must be created");
        for file in ["secret", "other/x", "dir/sub/y"] {
            std::fs::write(host.join(file), "x").expect("the host file must be written");
        }
        for (target, link) in [
            ("../secret", "dir/up"),
            ("../other", "dir/updir"),
            ("sub", "dir/in"),
            ("sub/../sub/y", "dir/twisty"),
            ("../escaped", "dir/dangle"),
            ("missing", "dir/nowhere"),
        ] {
            std::os::unix::fs::symlink(target, host.join(link)).expect("the link must be made");
        }
        Self(host)
    }

    /// A guest over this host directory as a Directory Mount, or as an
    /// Overlay's writable Directory Layer above an empty Memory.
    fn guest(&self, overlay: bool) -> Guest {
        let directory = Arc::new(Directory::new(&self.0).expect("the host directory must open"));
        let filesystem: Arc<dyn Filesystem> = if overlay {
            Arc::new(
                Overlay::new()
                    .layer(directory, Access::ReadWrite)
                    .layer(Arc::new(Memory::new()), Access::Read),
            )
        } else {
            directory
        };
        Guest::new(&[grant(filesystem, Access::ReadWrite, None)])
    }
}

#[cfg(unix)]
impl Drop for LinkedBeneath {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

// Each link names a real file outside `dir` yet inside the Mount, so only
// a link kept within the descriptor, not the Mount's root, is refused.
// @behavior RT-140
#[cfg(unix)]
#[test]
fn a_link_leading_out_of_an_opened_directory_is_not_permitted() {
    let host = LinkedBeneath::new("out");
    let answers = |overlay| {
        let mut guest = host.guest(overlay);
        [
            guest.call("open_following_beneath_dir", UP),
            guest.call("stat_following_beneath_dir", UP),
            guest.call("open_beneath_dir", THROUGH_UPDIR),
            guest.call("stat_following_beneath_dir", THROUGH_UPDIR),
            guest.call("create_following_beneath_dir", DANGLE),
        ]
    };
    assert_eq!(
        (
            answers(false),
            answers(true),
            host.0.join("escaped").exists()
        ),
        ([ERRNO_PERM; 5], [ERRNO_PERM; 5], false),
        "an open, a stat, or a create through preview1 of a path whose link leads out of a \
         directory opened beneath a Directory Mount, or an Overlay's Directory Layer, must answer \
         EPERM, as wasmtime-wasi does, and create nothing"
    );
}

// @behavior RT-141
#[cfg(unix)]
#[test]
fn a_link_staying_within_an_opened_directory_is_followed() {
    let host = LinkedBeneath::new("in");
    let answers = |overlay| {
        let mut guest = host.guest(overlay);
        [
            guest.call("open_following_beneath_dir", IN_LINKED),
            guest.call("open_following_beneath_dir", TWISTY),
        ]
    };
    assert_eq!(
        (answers(false), answers(true)),
        ([0; 2], [0; 2]),
        "an open through preview1 of a path whose links stay within a directory opened beneath \
         a Directory Mount, or an Overlay's Directory Layer, must reach the file they lead to"
    );
}

// The link's target lies within `dir`, where Linux would create it, so only
// the refusal to create through a link it cannot resolve answers ELOOP.
// @behavior RT-142
#[cfg(unix)]
#[test]
fn a_create_through_a_dangling_link_beneath_an_opened_directory_is_refused() {
    let host = LinkedBeneath::new("dangle");
    let answers = |overlay| {
        let errno = host
            .guest(overlay)
            .call("create_following_beneath_dir", NOWHERE);
        (errno, host.0.join("dir/missing").exists())
    };
    assert_eq!(
        (answers(false), answers(true)),
        ((ERRNO_LOOP, false), (ERRNO_LOOP, false)),
        "an open through preview1 creating a file through a link pointing at nothing beneath a \
         directory opened beneath a Directory Mount, or an Overlay's Directory Layer, must answer \
         ELOOP and create nothing"
    );
}

// The link names a file outside the host directory, so cap-std refuses it
// with no OS code, which wasmtime-wasi answers as EPERM.
// @behavior RT-143
#[cfg(unix)]
#[test]
fn a_link_leading_out_of_a_mount_is_not_permitted() {
    let host = LinkedBeneath::new("mount");
    let outside =
        std::env::temp_dir().join(format!("kobako-beneath-outside-{}", std::process::id()));
    std::fs::write(&outside, "host").expect("the outside file must be written");
    std::fs::remove_file(host.0.join("dir/up")).expect("the link must be removed");
    std::os::unix::fs::symlink(&outside, host.0.join("dir/up")).expect("the link must be made");
    let errno = host.guest(false).call("open_following", DIR_UP);
    let _ = std::fs::remove_file(&outside);
    assert_eq!(
        errno, ERRNO_PERM,
        "an open through preview1 from a Directory Mount's root of a link to a file outside its \
         host directory must answer EPERM, as wasmtime-wasi does"
    );
}

// Each file is open but lacks the access its call needs, so only the order
// of the bound and the access check decides between a trap and EBADF.
// @behavior RT-144
#[test]
fn a_buffer_outside_memory_is_bounded_before_a_files_access_is_checked() {
    let call = |name, rights| {
        Guest::new(&[grant(file_and_directory(), Access::ReadWrite, None)])
            .try_call_at(name, rights)
    };
    assert_eq!(
        [
            call("read_far_with", RIGHTS_FD_WRITE),
            call("write_far_with", RIGHTS_FD_READ),
            call("pwrite_far_with", RIGHTS_FD_READ),
            call("pread_far_with", RIGHTS_FD_WRITE),
        ],
        [None, None, None, Some(ERRNO_BADF)],
        "through preview1 on a file opened without the access each call needs, with a buffer \
         past memory, fd_read, fd_write, and fd_pwrite must trap, and fd_pread must answer \
         EBADF, as wasmtime-wasi does"
    );
}

const PUB_X: (i32, i32) = (1150, 5);

/// A Policy allowing `/data/pub/` alone, which swaps `pub` for a link to
/// `secret` the first time it is asked, as another writer could between
/// its judgment and the call.
#[cfg(unix)]
struct SwapsWhileAsked(std::path::PathBuf, std::sync::atomic::AtomicBool);

#[cfg(unix)]
impl Policy for SwapsWhileAsked {
    fn allows(&self, path: &str, _access: Access) -> bool {
        if self.1.swap(false, Ordering::SeqCst) {
            std::fs::rename(self.0.join("pub"), self.0.join("pub_old")).expect("pub must move");
            std::os::unix::fs::symlink("secret", self.0.join("pub"))
                .expect("the link must be made");
        }
        path.starts_with("/data/pub/")
    }
}

// The swap happens inside the Policy's own judgment, so only a call that
// reaches the path without following the link swapped in meanwhile is
// refused; a plain Directory would read `secret/x`.
// @behavior RT-145
#[cfg(unix)]
#[test]
fn a_link_swapped_in_after_a_policy_judges_a_path_is_not_followed() {
    let host = std::env::temp_dir().join(format!("kobako-swapped-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&host);
    for dir in ["pub", "secret"] {
        std::fs::create_dir_all(host.join(dir)).expect("the host directory must be created");
        std::fs::write(host.join(dir).join("x"), dir).expect("the host file must be written");
    }
    let policy = SwapsWhileAsked(host.clone(), std::sync::atomic::AtomicBool::new(true));
    let directory = Directory::new(&host).expect("the host directory must open");
    let errno = Guest::new(&[grant(
        Arc::new(directory),
        Access::Read,
        Some(Arc::new(policy)),
    )])
    .call("open_following", PUB_X);
    let _ = std::fs::remove_dir_all(&host);
    assert_eq!(
        errno, ERRNO_LOOP,
        "an open through preview1 beneath a Directory Mount with a Policy, of a path whose \
         directory another writer swaps for a link as the Policy judges it, must answer ELOOP"
    );
}
