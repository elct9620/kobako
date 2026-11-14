;; A guest whose one dispatch asks the host to yield, then answers that yield
;; with bytes the envelope cannot frame — something the real Guest Binary
;; never produces. An `eval` answers the yield with no bytes at all, a `run`
;; with a tag outside the live set, so each verb stages one of the two ways a
;; Yield Reply can be unframeable. The dispatch Reply is ignored and every
;; invocation ends with a nil Result.
;;
;; The version is the one in the import and export names; update it by
;; hand on a bump, same as `minimal_abi_ok.wat`.
(module
  (import "kobako:abi/host@0.6.0" "upcall" (func $upcall (param i32 i32 i32)))
  (memory (export "memory") 1)

  ;; The nil Result: the registry's ok tag 0x10, then nil 0xc0.
  (data (i32.const 8) "\10\c0")

  ;; A call upcall to `S#each` with a block and no arguments: tag 0x04, then
  ;; kind 0, target "S", method "each", block flag 1, and the payload `[[], {}]`.
  (data (i32.const 64) "\04\00\00\00\00\01S\00\00\00\04each\01\92\90\80")

  ;; A Yield Reply whose tag 0x05 is outside the live set.
  (data (i32.const 128) "\05\c0")

  ;; Return areas: at 992 the nil Result, at 1000 no answer at all, at 1008
  ;; the unknown Yield Reply.
  (data (i32.const 992) "\08\00\00\00\02\00\00\00")
  (data (i32.const 1000) "\00\00\00\00\00\00\00\00")
  (data (i32.const 1008) "\80\00\00\00\02\00\00\00")

  ;; 0 while an eval runs (empty answer), 1 while a run does (unknown tag).
  (global $unknown_tag (mut i32) (i32.const 0))

  (func $call_with_block
    (call $upcall (i32.const 64) (i32.const 19) (i32.const 984)))

  ;; Any room satisfies the host's reservations; nothing written there is
  ;; read back once its tag is.
  (func (export "cabi_realloc") (param i32 i32 i32 i32) (result i32) (i32.const 1024))

  ;; The request's first byte is its tag: 0x01 eval, 0x02 run, 0x03 yield.
  (func (export "kobako:abi/guest@0.6.0#downcall") (param $ptr i32) (param $len i32) (result i32)
    (local $tag i32)
    (local.set $tag (i32.load8_u (local.get $ptr)))
    (if (i32.eq (local.get $tag) (i32.const 0x03))
      (then
        (return
          (if (result i32) (global.get $unknown_tag)
            (then (i32.const 1008))
            (else (i32.const 1000))))))
    (global.set $unknown_tag (i32.eq (local.get $tag) (i32.const 0x02)))
    (call $call_with_block)
    (i32.const 992)))
