;; A guest whose every downcall stats `f` beneath the first preopen by its
;; path, opens it, stats what it opened, and lists the preopen. It answers an
;; array: the four errnos, the path's inode number, count of names, and three
;; times, the same five of the open file, then the inode number the listing
;; gives `f`. The real Guest Binary reads no listing, so no script can.
;;
;; Update the `i32.const` ABI version by hand on a bump, same as
;; `minimal_abi_ok.wat`.
(module
  (import "wasi_snapshot_preview1" "path_filestat_get"
    (func $path_stat (param i32 i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "path_open"
    (func $open (param i32 i32 i32 i32 i32 i64 i64 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_filestat_get"
    (func $fd_stat (param i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_readdir"
    (func $readdir (param i32 i32 i32 i64 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 700) "f")
  (func (export "__kobako_alloc") (param i32) (result i32) (i32.const 8192))

  ;; A msgpack uint64 at $at: its tag, then $value big-endian.
  (func $put (param $at i32) (param $value i64)
    (local $index i32)
    (i32.store8 (local.get $at) (i32.const 0xcf))
    (block $done
      (loop $next
        (br_if $done (i32.eq (local.get $index) (i32.const 8)))
        (i32.store8
          (i32.add (local.get $at) (i32.sub (i32.const 8) (local.get $index)))
          (i32.wrap_i64
            (i64.shr_u (local.get $value) (i64.extend_i32_u (i32.mul (local.get $index) (i32.const 8))))))
        (local.set $index (i32.add (local.get $index) (i32.const 1)))
        (br $next))))

  ;; The inode number, count of names, and three times of the filestat at
  ;; $stat, as five uint64s from $at.
  (func $put_stat (param $at i32) (param $stat i32)
    (call $put (local.get $at) (i64.load (i32.add (local.get $stat) (i32.const 8))))
    (call $put (i32.add (local.get $at) (i32.const 9)) (i64.load (i32.add (local.get $stat) (i32.const 24))))
    (call $put (i32.add (local.get $at) (i32.const 18)) (i64.load (i32.add (local.get $stat) (i32.const 40))))
    (call $put (i32.add (local.get $at) (i32.const 27)) (i64.load (i32.add (local.get $stat) (i32.const 48))))
    (call $put (i32.add (local.get $at) (i32.const 36)) (i64.load (i32.add (local.get $stat) (i32.const 56)))))

  ;; The inode number the listing at 1024 gives the one-byte name `f`. Each
  ;; dirent is a 24-byte header, inode number at 8 and name length at 16, then
  ;; its name.
  (func $listed (result i64)
    (local $at i32)
    (local $found i64)
    (local.set $at (i32.const 1024))
    (block $done
      (loop $next
        (br_if $done (i32.ge_u (local.get $at) (i32.add (i32.const 1024) (i32.load (i32.const 132)))))
        (if (i32.and
              (i32.eq (i32.load (i32.add (local.get $at) (i32.const 16))) (i32.const 1))
              (i32.eq (i32.load8_u (i32.add (local.get $at) (i32.const 24))) (i32.const 0x66)))
          (then (local.set $found (i64.load (i32.add (local.get $at) (i32.const 8))))))
        (local.set $at
          (i32.add (i32.add (local.get $at) (i32.const 24))
            (i32.load (i32.add (local.get $at) (i32.const 16)))))
        (br $next)))
    (local.get $found))

  ;; The answer at 8: the ok tag 0x10, an array of fifteen, then its members.
  (func (export "__kobako_downcall") (param i32 i32) (result i64)
    (i32.store8 (i32.const 10)
      (call $path_stat (i32.const 3) (i32.const 0) (i32.const 700) (i32.const 1) (i32.const 256)))
    ;; No oflags, fd_read and fd_filestat_get.
    (i32.store8 (i32.const 11)
      (call $open (i32.const 3) (i32.const 0) (i32.const 700) (i32.const 1) (i32.const 0)
        (i64.const 0x200002) (i64.const 0) (i32.const 0) (i32.const 128)))
    (i32.store8 (i32.const 12) (call $fd_stat (i32.load (i32.const 128)) (i32.const 320)))
    (i32.store8 (i32.const 13)
      (call $readdir (i32.const 3) (i32.const 1024) (i32.const 2048) (i64.const 0) (i32.const 132)))
    (i32.store8 (i32.const 8) (i32.const 0x10))
    (i32.store8 (i32.const 9) (i32.const 0x9f))
    (call $put_stat (i32.const 14) (i32.const 256))
    (call $put_stat (i32.const 59) (i32.const 320))
    (call $put (i32.const 104) (call $listed))
    (i64.or (i64.shl (i64.const 8) (i64.const 32)) (i64.const 105)))
  (func (export "__kobako_abi_version") (result i32) (i32.const 5)))
