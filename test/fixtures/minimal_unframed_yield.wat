;; A guest whose one dispatch asks the host to yield, then answers that yield
;; with bytes the envelope cannot frame — something the real Guest Binary
;; never produces. An `eval` answers the yield with no bytes at all, a `run`
;; with a tag outside the live set, so each verb stages one of the two ways a
;; Yield Reply can be unframeable. The dispatch Reply is ignored and every
;; invocation ends with a nil Result.
;;
;; Update the `i32.const` ABI version by hand on a bump, same as
;; `minimal_abi_ok.wat`.
(module
  (import "env" "__kobako_upcall" (func $upcall (param i32 i32) (result i64)))
  (memory (export "memory") 1)

  ;; The nil Result: the registry's ok tag 0x10, then nil 0xc0.
  (data (i32.const 8) "\10\c0")

  ;; A call upcall to `S#each` with a block and no arguments: tag 0x04, then
  ;; kind 0, target "S", method "each", block flag 1, and the payload `[[], {}]`.
  (data (i32.const 64) "\04\00\00\00\00\01S\00\00\00\04each\01\92\90\80")

  ;; A Yield Reply whose tag 0x05 is outside the live set.
  (data (i32.const 128) "\05\c0")

  ;; 0 while an eval runs (empty answer), 1 while a run does (unknown tag).
  (global $unknown_tag (mut i32) (i32.const 0))

  (func $call_with_block
    (drop (call $upcall (i32.const 64) (i32.const 19))))

  (func $nil (result i64)
    (i64.or (i64.shl (i64.const 8) (i64.const 32)) (i64.const 2)))

  ;; Any non-zero offset satisfies the host's reservations; nothing written
  ;; there is read back once its tag is.
  (func (export "__kobako_alloc") (param i32) (result i32) (i32.const 1024))

  ;; The request's first byte is its tag: 0x01 eval, 0x02 run, 0x03 yield.
  (func (export "__kobako_downcall") (param $ptr i32) (param $len i32) (result i64)
    (local $tag i32)
    (local.set $tag (i32.load8_u (local.get $ptr)))
    (if (i32.eq (local.get $tag) (i32.const 0x03))
      (then
        (return
          (if (result i64) (global.get $unknown_tag)
            (then (i64.or (i64.shl (i64.const 128) (i64.const 32)) (i64.const 2)))
            (else (i64.const 0))))))
    (global.set $unknown_tag (i32.eq (local.get $tag) (i32.const 0x02)))
    (call $call_with_block)
    (call $nil))

  (func (export "__kobako_abi_version") (result i32) (i32.const 4)))
