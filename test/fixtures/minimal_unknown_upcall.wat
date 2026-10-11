;; A guest that sends the host an upcall under a tag no host serves, then
;; reports what came back: an `eval` ends with `true` when the host answered
;; on the unsupported arm and `false` otherwise. The real Guest Binary only
;; ever sends a call, so this stands in for a guest newer than its host.
;;
;; Update the `i32.const` ABI version by hand on a bump, same as
;; `minimal_abi_ok.wat`.
(module
  (import "env" "__kobako_upcall" (func $upcall (param i32 i32) (result i64)))
  (memory (export "memory") 1)

  ;; The two Results: the registry's ok tag 0x10, then msgpack true / false.
  (data (i32.const 8) "\10\c3")
  (data (i32.const 12) "\10\c2")

  ;; An upcall under tag 0x7e, unassigned in the registry.
  (data (i32.const 64) "\7e")

  (func (export "__kobako_alloc") (param i32) (result i32) (i32.const 1024))

  (func (export "__kobako_downcall") (param $ptr i32) (param $len i32) (result i64)
    (local $answer i64)
    (local.set $answer (call $upcall (i32.const 64) (i32.const 1)))
    ;; The answer's pointer is the high 32 bits; its first byte is the arm.
    (if (result i64)
      (i32.and
        (i64.ne (local.get $answer) (i64.const 0))
        (i32.eq
          (i32.load8_u (i32.wrap_i64 (i64.shr_u (local.get $answer) (i64.const 32))))
          (i32.const 0x15)))
      (then (i64.or (i64.shl (i64.const 8) (i64.const 32)) (i64.const 2)))
      (else (i64.or (i64.shl (i64.const 12) (i64.const 32)) (i64.const 2)))))

  (func (export "__kobako_abi_version") (result i32) (i32.const 4)))
