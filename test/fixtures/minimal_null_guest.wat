;; A guest that satisfies the whole ABI and does nothing else: every
;; downcall ignores its request and answers with the same nil Result. Its
;; purpose is measurement, not coverage — a Sandbox driven against it pays
;; the host's per-invocation cost and nothing more, so
;; benchmark/host_invocation.rb reads that cost as a total instead of
;; deriving it by subtracting a guest budget from one (a subtraction of two
;; near-equal milliseconds that loses all its significant digits).
;;
;; Text format on purpose — the ext enables wasmtime's `wat` feature, so
;; this loads through the same `wasm_path:` path as a binary artifact.
;; The version is the one in the export name; update it by hand on a bump.
(module
  (memory (export "memory") 1)

  ;; The Outcome for "the invocation returned nil": the registry's ok tag
  ;; 0x10, then the payload adapter's nil, 0xc0.
  (data (i32.const 8) "\10\c0")

  ;; The return area at 992 naming the answer above: place 8, length 2.
  (data (i32.const 992) "\08\00\00\00\02\00\00\00")

  ;; Any room satisfies the host's request reservation; the bytes written
  ;; there are never read back.
  (func (export "cabi_realloc") (param i32 i32 i32 i32) (result i32) (i32.const 1024))

  (func (export "kobako:abi/guest@0.6.0#downcall") (param i32 i32) (result i32)
    (i32.const 992)))
