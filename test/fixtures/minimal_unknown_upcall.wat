;; A guest that sends the host an upcall under a tag no host serves, then
;; reports what came back: an `eval` ends with `true` when the host answered
;; on the unsupported arm and `false` otherwise. The real Guest Binary only
;; ever sends a call, so this stands in for a guest newer than its host.
;;
;; The version is the one in the import and export names; update it by
;; hand on a bump, same as `minimal_abi_ok.wat`.
(module
  (import "kobako:abi/host@0.6.0" "upcall" (func $upcall (param i32 i32 i32)))
  (memory (export "memory") 1)

  ;; The two Results: the registry's ok tag 0x10, then msgpack true / false.
  (data (i32.const 8) "\10\c3")
  (data (i32.const 12) "\10\c2")

  ;; Return areas naming each Result: at 992 true, at 1000 false.
  (data (i32.const 992) "\08\00\00\00\02\00\00\00")
  (data (i32.const 1000) "\0c\00\00\00\02\00\00\00")

  ;; An upcall under tag 0x7e, unassigned in the registry.
  (data (i32.const 64) "\7e")

  (func (export "cabi_realloc") (param i32 i32 i32 i32) (result i32) (i32.const 1024))

  (func (export "kobako:abi/guest@0.6.0#downcall") (param $ptr i32) (param $len i32) (result i32)
    ;; The host names its answer in the return area at 984; the answer's
    ;; first byte is its arm.
    (call $upcall (i32.const 64) (i32.const 1) (i32.const 984))
    (if (result i32)
      (i32.and
        (i32.ne (i32.load (i32.const 988)) (i32.const 0))
        (i32.eq (i32.load8_u (i32.load (i32.const 984))) (i32.const 0x15)))
      (then (i32.const 992))
      (else (i32.const 1000)))))
