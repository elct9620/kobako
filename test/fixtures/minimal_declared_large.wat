;; Minimal wasm32 module declaring a 2 MiB memory and a table of 262144
;; entries, 2 MiB on a 64-bit host, both past a 1 MiB memory cap. Its run
;; answers nothing, so it fails past instantiation without a result.
;; The version is the one in the export name; update it by hand on a bump.
(module
  (memory (export "memory") 32)
  (table 262144 funcref)
  ;; Any room satisfies the host's request reservation.
  (func (export "cabi_realloc") (param i32 i32 i32 i32) (result i32) (i32.const 1024))
  (func (export "kobako:abi/guest@0.6.0#downcall") (param i32 i32) (result i32) (i32.const 0)))
