;; A guest whose compile answers what it read from the ambient sources: one
;; bytecode holding the realtime clock, then eight bytes of entropy. Every
;; other downcall answers a nil Result at once.
;;
;; The version is the one in the export name; update it by hand on a bump,
;; same as `minimal_abi_ok.wat`.
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

  ;; Return areas: at 992 the nil Result, at 1000 the 25-byte compile reply.
  (data (i32.const 992) "\08\00\00\00\02\00\00\00")
  (data (i32.const 1000) "\17\00\00\00\19\00\00\00")

  ;; Any room satisfies the host's reservations; nothing written there is
  ;; read back once its tag is.
  (func (export "cabi_realloc") (param i32 i32 i32 i32) (result i32) (i32.const 1024))

  ;; The request's first byte is its tag: 0x08 compile.
  (func (export "kobako:abi/guest@0.6.0#downcall") (param $ptr i32) (param $len i32) (result i32)
    (if (i32.eq (i32.load8_u (local.get $ptr)) (i32.const 0x08))
      (then
        (drop (call $clock (i32.const 0) (i64.const 1) (i32.const 32)))
        (drop (call $random (i32.const 40) (i32.const 8)))
        (return (i32.const 1000))))
    (i32.const 992)))
