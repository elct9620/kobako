;; A guest that answers its invocation without an Outcome the envelope can
;; frame — which the real Guest Binary never does. An `eval` downcall is
;; answered with no bytes; a `run` downcall with a tag outside the live set.
;; The version is the one in the export name; update it by hand on a bump,
;; same as `minimal_abi_ok.wat`.
(module
  (memory (export "memory") 1)

  (data (i32.const 8) "\7f\2a")

  ;; Return areas: at 992 no answer at all, at 1000 the two bytes at 8.
  (data (i32.const 992) "\00\00\00\00\00\00\00\00")
  (data (i32.const 1000) "\08\00\00\00\02\00\00\00")

  (func (export "cabi_realloc") (param i32 i32 i32 i32) (result i32) (i32.const 1024))

  ;; The request's first byte is its tag: 0x01 eval, 0x02 run.
  (func (export "kobako:abi/guest@0.6.0#downcall") (param $ptr i32) (param $len i32) (result i32)
    (if (result i32) (i32.eq (i32.load8_u (local.get $ptr)) (i32.const 0x01))
      (then (i32.const 992))
      (else (i32.const 1000)))))
