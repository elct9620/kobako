;; Minimal wasm32 module whose run fills its 64 MiB memory with entropy over
;; and over, never answering; only a deadline ends it.
;; The version is the one in the export name; update it by hand on a bump.
(module
  (import "wasi_snapshot_preview1" "random_get" (func $random (param i32 i32) (result i32)))
  (memory (export "memory") 1024)
  ;; Any room satisfies the host's request reservation.
  (func (export "cabi_realloc") (param i32 i32 i32 i32) (result i32) (i32.const 1024))
  (func (export "kobako:abi/guest@0.6.0#downcall") (param i32 i32) (result i32)
    (loop $again
      (drop (call $random (i32.const 0) (i32.const 67108864)))
      (br $again))
    (i32.const 0)))
