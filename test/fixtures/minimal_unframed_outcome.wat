;; A guest that answers its invocation without an Outcome the envelope can
;; frame — which the real Guest Binary never does. An `eval` downcall is
;; answered with no bytes; a `run` downcall with a tag outside the live set.
;; Update the `i32.const` ABI version by hand on a bump, same as
;; `minimal_abi_ok.wat`.
(module
  (memory (export "memory") 1)

  (data (i32.const 8) "\7f\2a")

  (func (export "__kobako_alloc") (param i32) (result i32) (i32.const 1024))

  ;; The request's first byte is its tag: 0x01 eval, 0x02 run.
  (func (export "__kobako_downcall") (param $ptr i32) (param $len i32) (result i64)
    (if (result i64) (i32.eq (i32.load8_u (local.get $ptr)) (i32.const 0x01))
      (then (i64.const 0))
      (else (i64.or (i64.shl (i64.const 8) (i64.const 32)) (i64.const 2)))))

  (func (export "__kobako_abi_version") (result i32) (i32.const 5)))
