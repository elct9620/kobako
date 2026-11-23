;; Minimal wasm32 module whose global holds a GC array of 262144 i64s
;; (2 MiB), so instantiating it grows the collected heap past a 1 MiB memory
;; cap before any script runs.
;; The version is the one in the export name; update it by hand on a bump.
(module
  (type $words (array i64))
  (global (ref $words) (array.new $words (i64.const 7) (i32.const 262144)))
  (memory (export "memory") 1)
  ;; Any room satisfies the host's request reservation.
  (func (export "cabi_realloc") (param i32 i32 i32 i32) (result i32) (i32.const 1024))
  (func (export "kobako:abi/guest@0.6.0#downcall") (param i32 i32) (result i32) (i32.const 0)))
