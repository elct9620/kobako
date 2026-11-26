;; Minimal wasm32 module whose run opens `data` beneath the first preopen
;; once, so a mount there hears the flood begin, then fills its 64 MiB memory
;; with entropy over and over, never answering; only a deadline ends it.
;; The version is the one in the export name; update it by hand on a bump.
(module
  (import "wasi_snapshot_preview1" "path_open"
    (func $open (param i32 i32 i32 i32 i32 i64 i64 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "random_get" (func $random (param i32 i32) (result i32)))
  (memory (export "memory") 1024)
  (data (i32.const 700) "data")
  ;; Any room satisfies the host's request reservation.
  (func (export "cabi_realloc") (param i32 i32 i32 i32) (result i32) (i32.const 1024))
  (func (export "kobako:abi/guest@0.6.0#downcall") (param i32 i32) (result i32)
    ;; O_DIRECTORY with fd_read; what it answers plays no part.
    (drop (call $open (i32.const 3) (i32.const 0) (i32.const 700) (i32.const 4)
      (i32.const 2) (i64.const 2) (i64.const 0) (i32.const 0) (i32.const 128)))
    (loop $again
      (drop (call $random (i32.const 0) (i32.const 67108864)))
      (br $again))
    (i32.const 0)))
