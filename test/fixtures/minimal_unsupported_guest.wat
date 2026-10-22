;; A guest that serves neither an invocation nor a block, answering each as
;; unsupported — which the real Guest Binary never does for an invocation.
;; An `eval` is answered as unsupported outright. A `run` first calls `S#each`
;; with a block, so the host's yield reaches the guest and is answered as
;; unsupported, then the run ends with a nil Result.
;;
;; Update the `i32.const` ABI version by hand on a bump, same as
;; `minimal_abi_ok.wat`.
(module
  (import "env" "__kobako_upcall" (func $upcall (param i32 i32) (result i64)))
  (memory (export "memory") 1)

  ;; The nil Result: the registry's ok tag 0x10, then nil 0xc0.
  (data (i32.const 8) "\10\c0")

  ;; The unsupported answer: tag 0x15, kind 0, then an empty message.
  (data (i32.const 16) "\15\00\00\00\00\00")

  ;; A call upcall to `S#each` with a block and no arguments: tag 0x04, then
  ;; kind 0, target "S", method "each", block flag 1, and the payload `[[], {}]`.
  (data (i32.const 64) "\04\00\00\00\00\01S\00\00\00\04each\01\92\90\80")

  (func $unsupported (result i64)
    (i64.or (i64.shl (i64.const 16) (i64.const 32)) (i64.const 6)))

  ;; Any non-zero offset satisfies the host's reservations; nothing written
  ;; there is read back once its tag is.
  (func (export "__kobako_alloc") (param i32) (result i32) (i32.const 1024))

  ;; The request's first byte is its tag: 0x02 run; every other is refused.
  (func (export "__kobako_downcall") (param $ptr i32) (param $len i32) (result i64)
    (if (i32.eq (i32.load8_u (local.get $ptr)) (i32.const 0x02))
      (then
        (drop (call $upcall (i32.const 64) (i32.const 19)))
        (return (i64.or (i64.shl (i64.const 8) (i64.const 32)) (i64.const 2)))))
    (call $unsupported))

  (func (export "__kobako_abi_version") (result i32) (i32.const 5)))
