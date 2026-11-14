;; Minimal wasm32 module that passes the construction-time ABI version
;; check but whose `cabi_realloc` always traps, as a guest with no room
;; left does — the frozen witness for a host that cannot reserve guest
;; memory for the downcall, surfacing as `Kobako::TrapError`.
;; The version is the one in the export name; update it by hand on a bump.
(module
  (memory (export "memory") 1)
  (func (export "cabi_realloc") (param i32 i32 i32 i32) (result i32) unreachable)
  (func (export "kobako:abi/guest@0.6.0#downcall") (param i32 i32) (result i32) (i32.const 0)))
