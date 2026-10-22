;; A guest whose compile answers what it read from the ambient sources: one
;; bytecode holding the realtime clock, then eight bytes of entropy. Every
;; other downcall answers a nil Result at once.
;;
;; Update the `i32.const` ABI version by hand on a bump, same as
;; `minimal_abi_ok.wat`.
(module
  (import "wasi_snapshot_preview1" "clock_time_get"
    (func $clock (param i32 i64 i32) (result i32)))
  (import "wasi_snapshot_preview1" "random_get"
    (func $random (param i32 i32) (result i32)))
  (memory (export "memory") 1)

  ;; The nil Result: the registry's ok tag 0x10, then nil 0xc0.
  (data (i32.const 8) "\10\c0")

  ;; The compile reply's head: the ok tag 0x10, one bytecode, sixteen bytes
  ;; long. It starts at 23 so the clock lands at 32, where preview1 accepts
  ;; a timestamp only on an 8-byte boundary, and the entropy at 40.
  (data (i32.const 23) "\10\00\00\00\01\00\00\00\10")

  ;; Any non-zero offset satisfies the host's reservations; nothing written
  ;; there is read back once its tag is.
  (func (export "__kobako_alloc") (param i32) (result i32) (i32.const 1024))

  ;; The request's first byte is its tag: 0x08 compile.
  (func (export "__kobako_downcall") (param $ptr i32) (param $len i32) (result i64)
    (if (i32.eq (i32.load8_u (local.get $ptr)) (i32.const 0x08))
      (then
        (drop (call $clock (i32.const 0) (i64.const 1) (i32.const 32)))
        (drop (call $random (i32.const 40) (i32.const 8)))
        (return (i64.or (i64.shl (i64.const 23) (i64.const 32)) (i64.const 25)))))
    (i64.or (i64.shl (i64.const 8) (i64.const 32)) (i64.const 2)))

  (func (export "__kobako_abi_version") (result i32) (i32.const 5)))
