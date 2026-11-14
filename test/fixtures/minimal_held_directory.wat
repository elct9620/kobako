;; A guest whose every downcall opens `dir` beneath the first preopen as a
;; directory, then opens `inner` through the descriptor it opened to read,
;; and answers that open's errno. The real Guest Binary holds no directory
;; open, so no script can open through one.
;;
;; The version is the one in the export name; update it by hand on a bump,
;; same as `minimal_abi_ok.wat`.
(module
  (import "wasi_snapshot_preview1" "path_open"
    (func $open (param i32 i32 i32 i32 i32 i64 i64 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 700) "dir")
  (data (i32.const 720) "inner")
  ;; The return area at 992 naming the two-byte answer at 8.
  (data (i32.const 992) "\08\00\00\00\02\00\00\00")
  (func (export "cabi_realloc") (param i32 i32 i32 i32) (result i32) (i32.const 1024))
  ;; The answer at 8: the ok tag 0x10, then the errno as a positive fixint.
  (func (export "kobako:abi/guest@0.6.0#downcall") (param i32 i32) (result i32)
    (local $errno i32)
    ;; O_DIRECTORY with fd_read, so the directory opens to read rather than
    ;; standing only for its path, and passes every right down.
    (local.set $errno (call $open (i32.const 3) (i32.const 0) (i32.const 700) (i32.const 3)
      (i32.const 2) (i64.const 2) (i64.const 0x1FFFFFFF) (i32.const 0) (i32.const 128)))
    ;; No oflags, fd_read alone.
    (if (i32.eqz (local.get $errno))
      (then (local.set $errno (call $open (i32.load (i32.const 128)) (i32.const 0)
        (i32.const 720) (i32.const 5) (i32.const 0) (i64.const 2) (i64.const 0)
        (i32.const 0) (i32.const 132)))))
    (i32.store8 (i32.const 8) (i32.const 0x10))
    (i32.store8 (i32.const 9) (local.get $errno))
    (i32.const 992)))
