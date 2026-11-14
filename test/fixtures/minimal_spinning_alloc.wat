;; Minimal wasm32 module that passes the construction-time ABI version
;; check but whose `cabi_realloc` never returns, so an invocation spins
;; while the host is still writing its request — before the export call.
;; The version is the one in the export name; update it by hand on a bump.
(module
  (memory (export "memory") 1)
  (func (export "cabi_realloc") (param i32 i32 i32 i32) (result i32)
    (loop $spin (br $spin))
    (i32.const 0))
  (func (export "kobako:abi/guest@0.6.0#downcall") (param i32 i32) (result i32) (i32.const 0)))
