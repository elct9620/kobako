;; Minimal wasm32 module that passes the construction-time ABI version
;; check but whose `__kobako_alloc` never returns, so an invocation spins
;; while the host is still writing its request — before the export call.
(module
  (memory (export "memory") 1)
  (func (export "__kobako_alloc") (param i32) (result i32)
    (loop $spin (br $spin))
    (i32.const 0))
  (func (export "__kobako_downcall") (param i32 i32) (result i64) (i64.const 0))
  (func (export "__kobako_abi_version") (result i32) (i32.const 5)))
