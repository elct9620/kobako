;; Minimal wasm32 module whose start function grows its memory by 32 pages
;; (2 MiB), past a 1 MiB memory cap, before any script runs.
;; The version is the one in the export name; update it by hand on a bump.
(module
  (memory (export "memory") 1)
  (func $grow (drop (memory.grow (i32.const 32))))
  (start $grow)
  ;; Any room satisfies the host's request reservation.
  (func (export "cabi_realloc") (param i32 i32 i32 i32) (result i32) (i32.const 1024))
  (func (export "kobako:abi/guest@0.6.0#downcall") (param i32 i32) (result i32) (i32.const 0)))
