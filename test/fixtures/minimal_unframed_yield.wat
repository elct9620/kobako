;; A guest whose one dispatch asks the host to yield, then answers that yield
;; with bytes the envelope cannot frame — something the real Guest Binary
;; never produces. `__kobako_eval` answers with no bytes at all,
;; `__kobako_run` with a tag outside the live set, so each verb stages one of
;; the two ways a Yield Reply can be unframeable. The dispatch Reply is
;; ignored and every invocation ends with a nil Result.
;;
;; Update the `i32.const` ABI version by hand on a bump, same as
;; `minimal_abi_ok.wat`.
(module
  (import "env" "__kobako_dispatch" (func $dispatch (param i32 i32) (result i64)))
  (memory (export "memory") 1)

  ;; The nil Result: the fixed layout's result tag 0x01, then nil 0xc0.
  (data (i32.const 8) "\01\c0")

  ;; A constant-path Call to `S#each` with a block and no arguments: kind
  ;; 0, target "S", method "each", block flag 1, then the payload `[[], {}]`.
  (data (i32.const 64) "\00\00\00\00\01S\00\00\00\04each\01\92\90\80")

  ;; A Yield Reply whose tag 0x05 is outside the live set {0x01, 0x02, 0x04}.
  (data (i32.const 128) "\05\c0")

  ;; 0 while an #eval runs (empty answer), 1 while a #run does (unknown tag).
  (global $unknown_tag (mut i32) (i32.const 0))

  (func $call_with_block
    (drop (call $dispatch (i32.const 64) (i32.const 18))))

  (func (export "__kobako_eval")
    (global.set $unknown_tag (i32.const 0))
    (call $call_with_block))

  (func (export "__kobako_run") (param i32 i32)
    (global.set $unknown_tag (i32.const 1))
    (call $call_with_block))

  (func (export "__kobako_yield_to_block") (param i32 i32) (result i64)
    (if (result i64) (global.get $unknown_tag)
      (then (i64.or (i64.shl (i64.const 128) (i64.const 32)) (i64.const 2)))
      (else (i64.const 0))))

  ;; Any non-zero offset satisfies the host's reservations; nothing written
  ;; there is read back.
  (func (export "__kobako_alloc") (param i32) (result i32) (i32.const 1024))

  (func (export "__kobako_take_outcome") (result i64)
    (i64.or (i64.shl (i64.const 8) (i64.const 32)) (i64.const 2)))

  (func (export "__kobako_abi_version") (result i32) (i32.const 3)))
