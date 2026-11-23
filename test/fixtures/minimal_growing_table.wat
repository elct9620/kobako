;; Minimal wasm32 module whose run grows a table by 262144 entries, 2 MiB on
;; a 64-bit host, past a 1 MiB memory cap. The cap ends the run; it would
;; otherwise answer nothing.
;; The version is the one in the export name; update it by hand on a bump.
(module
  (memory (export "memory") 1)
  (table $t 0 funcref)
  ;; Any room satisfies the host's request reservation.
  (func (export "cabi_realloc") (param i32 i32 i32 i32) (result i32) (i32.const 1024))
  (func (export "kobako:abi/guest@0.6.0#downcall") (param i32 i32) (result i32)
    (drop (table.grow $t (ref.null func) (i32.const 262144)))
    (i32.const 0)))
