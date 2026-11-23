;; Minimal wasm32 module whose run asks for 64 MiB of entropy into a buffer
;; that starts in its one-page memory and ends far past it.
;; The version is the one in the export name; update it by hand on a bump.
(module
  (import "wasi_snapshot_preview1" "random_get" (func $random (param i32 i32) (result i32)))
  (memory (export "memory") 1)
  ;; Any room satisfies the host's request reservation.
  (func (export "cabi_realloc") (param i32 i32 i32 i32) (result i32) (i32.const 1024))
  (func (export "kobako:abi/guest@0.6.0#downcall") (param i32 i32) (result i32)
    (drop (call $random (i32.const 0) (i32.const 67108864)))
    (i32.const 0)))
