;; A guest whose every downcall opens `dir` beneath the first preopen as a
;; directory, renames `dir` to `moved`, makes another `dir`, then stats
;; `inner` through the descriptor it opened, and answers that stat's
;; errno. The real Guest Binary holds no directory open, so no script can
;; look up through one.
;;
;; Update the `i32.const` ABI version by hand on a bump, same as
;; `minimal_abi_ok.wat`.
(module
  (import "wasi_snapshot_preview1" "path_open"
    (func $open (param i32 i32 i32 i32 i32 i64 i64 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "path_rename"
    (func $rename (param i32 i32 i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "path_create_directory"
    (func $mkdir (param i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "path_filestat_get"
    (func $stat (param i32 i32 i32 i32 i32) (result i32)))
  (memory (export "memory") 1)

  (data (i32.const 700) "dir")
  (data (i32.const 710) "moved")
  (data (i32.const 720) "inner")

  (func (export "__kobako_alloc") (param i32) (result i32) (i32.const 1024))

  ;; The answer at 8: the ok tag 0x10, then the errno as a positive fixint.
  (func (export "__kobako_downcall") (param i32 i32) (result i64)
    (local $errno i32)
    ;; O_DIRECTORY, every right but none a directory refuses.
    (local.set $errno (call $open (i32.const 3) (i32.const 0) (i32.const 700) (i32.const 3)
      (i32.const 2) (i64.const 0) (i64.const 0x1FFFFFFF) (i32.const 0) (i32.const 128)))
    (if (i32.eqz (local.get $errno))
      (then (local.set $errno (call $rename (i32.const 3) (i32.const 700) (i32.const 3)
        (i32.const 3) (i32.const 710) (i32.const 5)))))
    (if (i32.eqz (local.get $errno))
      (then (local.set $errno (call $mkdir (i32.const 3) (i32.const 700) (i32.const 3)))))
    (if (i32.eqz (local.get $errno))
      (then (local.set $errno (call $stat (i32.load (i32.const 128)) (i32.const 0)
        (i32.const 720) (i32.const 5) (i32.const 256)))))
    (i32.store8 (i32.const 8) (i32.const 0x10))
    (i32.store8 (i32.const 9) (local.get $errno))
    (i64.or (i64.shl (i64.const 8) (i64.const 32)) (i64.const 2)))

  (func (export "__kobako_abi_version") (result i32) (i32.const 5)))
