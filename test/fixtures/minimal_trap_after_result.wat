;; A guest whose Outcome buffer already holds a well-formed nil Result,
;; yet whose entry points trap. It is `minimal_null_guest.wat` with the
;; entries replaced by `unreachable`, so a host that read the buffer
;; after a trap would answer nil where the invocation actually trapped.
;; Update the `i32.const` ABI version by hand on a bump, same as
;; `minimal_abi_ok.wat`.
(module
  (memory (export "memory") 1)

  (data (i32.const 8) "\01\c0")

  (func (export "__kobako_eval") unreachable)
  (func (export "__kobako_run") (param i32 i32) unreachable)

  (func (export "__kobako_alloc") (param i32) (result i32) (i32.const 1024))

  (func (export "__kobako_take_outcome") (result i64)
    (i64.or (i64.shl (i64.const 8) (i64.const 32)) (i64.const 2)))

  (func (export "__kobako_abi_version") (result i32) (i32.const 3)))
