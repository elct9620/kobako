;; A guest whose every downcall answers the same five-element Array, six
;; values in all, without growing its memory. A Sandbox whose memory cap
;; allows fewer values than that must refuse the answer, which the real
;; Guest Binary cannot be made to send: it spends more memory encoding a
;; value than the budget grants it.
;;
;; Text format on purpose — the ext enables wasmtime's `wat` feature, so
;; this loads through the same `wasm_path:` path as a binary artifact.
;; Update the `i32.const` ABI version by hand on a bump, same as
;; `minimal_abi_ok.wat`.
(module
  (memory (export "memory") 1)

  ;; The ok tag 0x10, then [0, 0, 0, 0, 0] as fixarray 0x95 of five
  ;; positive fixints.
  (data (i32.const 8) "\10\95\00\00\00\00\00")

  ;; Any non-zero offset satisfies the host's request reservation; the
  ;; bytes written there are never read back.
  (func (export "__kobako_alloc") (param i32) (result i32) (i32.const 1024))

  ;; (ptr << 32) | len over the constant answer above.
  (func (export "__kobako_downcall") (param i32 i32) (result i64)
    (i64.or (i64.shl (i64.const 8) (i64.const 32)) (i64.const 7)))

  (func (export "__kobako_abi_version") (result i32) (i32.const 5)))
