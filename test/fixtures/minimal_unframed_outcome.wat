;; A guest that returns from its entry points without an Outcome the
;; envelope can frame — which the real Guest Binary never does. `#eval`
;; leaves the buffer empty; `#run` leaves a tag outside the live set.
;; Update the `i32.const` ABI version by hand on a bump, same as
;; `minimal_abi_ok.wat`.
(module
  (memory (export "memory") 1)

  (data (i32.const 8) "\7f\2a")

  (global $outcome (mut i64) (i64.const 0))

  (func (export "__kobako_eval") (global.set $outcome (i64.const 0)))
  (func (export "__kobako_run") (param i32 i32)
    (global.set $outcome
      (i64.or (i64.shl (i64.const 8) (i64.const 32)) (i64.const 2))))

  (func (export "__kobako_alloc") (param i32) (result i32) (i32.const 1024))

  (func (export "__kobako_take_outcome") (result i64) (global.get $outcome))

  (func (export "__kobako_abi_version") (result i32) (i32.const 3)))
