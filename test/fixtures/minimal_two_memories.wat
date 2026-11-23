;; Minimal wasm32 module whose run grows each of its two memories by 12
;; pages (768 KiB), so only their sum passes a 1 MiB memory cap. The cap
;; ends the run; it would otherwise answer nothing.
;; The version is the one in the export name; update it by hand on a bump.
(module
  (memory $a (export "memory") 1)
  (memory $b 0)
  ;; Any room satisfies the host's request reservation.
  (func (export "cabi_realloc") (param i32 i32 i32 i32) (result i32) (i32.const 1024))
  (func (export "kobako:abi/guest@0.6.0#downcall") (param i32 i32) (result i32)
    (drop (memory.grow $a (i32.const 12)))
    (drop (memory.grow $b (i32.const 12)))
    (i32.const 0)))
