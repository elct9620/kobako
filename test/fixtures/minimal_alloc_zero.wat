;; Minimal wasm32 module that passes the B-40 construction-time ABI
;; version check but whose `__kobako_alloc` always reports exhaustion
;; (returns 0) — the frozen witness for the docs/behavior/errors.md
;; E-31 branch: the host cannot reserve the #run invocation envelope,
;; a runtime-intact failure surfacing as `Kobako::SandboxError`.
(module
  (memory (export "memory") 1)
  (func (export "__kobako_alloc") (param i32) (result i32) (i32.const 0))
  (func (export "__kobako_eval"))
  (func (export "__kobako_run") (param i32 i32))
  (func (export "__kobako_abi_version") (result i32) (i32.const 2)))
