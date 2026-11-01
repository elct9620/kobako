;; A guest whose every downcall syncs the first preopen and answers the sync's
;; errno. The real Guest Binary imports no fd_sync, so no script can sync a
;; directory.
;;
;; Update the `i32.const` ABI version by hand on a bump, same as
;; `minimal_abi_ok.wat`.
(module
  (import "wasi_snapshot_preview1" "fd_sync" (func $sync (param i32) (result i32)))
  (memory (export "memory") 1)

  (func (export "__kobako_alloc") (param i32) (result i32) (i32.const 1024))

  ;; The answer at 8: the ok tag 0x10, then the errno as a positive fixint.
  (func (export "__kobako_downcall") (param i32 i32) (result i64)
    (i32.store8 (i32.const 8) (i32.const 0x10))
    (i32.store8 (i32.const 9) (call $sync (i32.const 3)))
    (i64.or (i64.shl (i64.const 8) (i64.const 32)) (i64.const 2)))

  (func (export "__kobako_abi_version") (result i32) (i32.const 5)))
