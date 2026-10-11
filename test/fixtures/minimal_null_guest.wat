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
;; Update the `i32.const` ABI version by hand on a bump, same as
;; `minimal_abi_ok.wat`.
(module
  (memory (export "memory") 1)

  ;; The Outcome for "the invocation returned nil": the registry's ok tag
  ;; 0x10, then the payload adapter's nil, 0xc0.
  (data (i32.const 8) "\10\c0")

  ;; Any non-zero offset satisfies the host's request reservation; the
  ;; bytes written there are never read back.
  (func (export "__kobako_alloc") (param i32) (result i32) (i32.const 1024))

  ;; (ptr << 32) | len over the constant answer above.
  (func (export "__kobako_downcall") (param i32 i32) (result i64)
    (i64.or (i64.shl (i64.const 8) (i64.const 32)) (i64.const 2)))

  (func (export "__kobako_abi_version") (result i32) (i32.const 4)))
