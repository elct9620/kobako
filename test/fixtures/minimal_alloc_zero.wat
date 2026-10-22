;; Minimal wasm32 module that passes the construction-time ABI version
;; check but whose `__kobako_alloc` always reports exhaustion (returns 0)
;; — the frozen witness for a host that cannot reserve guest memory for
;; the downcall, a runtime-intact failure surfacing as
;; `Kobako::SandboxError`.
(module
  (memory (export "memory") 1)
  (func (export "__kobako_alloc") (param i32) (result i32) (i32.const 0))
  (func (export "__kobako_downcall") (param i32 i32) (result i64) (i64.const 0))
  (func (export "__kobako_abi_version") (result i32) (i32.const 5)))
