;; A guest whose every downcall answers the same five-element Array, six
;; values in all, without growing its memory. A Sandbox whose memory cap
;; allows fewer values than that must refuse the answer, which the real
;; Guest Binary cannot be made to send: it spends more memory encoding a
;; value than the budget grants it.
;;
;; Text format on purpose — the ext enables wasmtime's `wat` feature, so
;; this loads through the same `wasm_path:` path as a binary artifact.
;; The version is the one in the export name; update it by hand on a bump.
(module
  (memory (export "memory") 1)

  ;; The ok tag 0x10, then [0, 0, 0, 0, 0] as fixarray 0x95 of five
  ;; positive fixints.
  (data (i32.const 8) "\10\95\00\00\00\00\00")

  ;; The return area at 992 naming the answer above: place 8, length 7.
  (data (i32.const 992) "\08\00\00\00\07\00\00\00")

  ;; Any room satisfies the host's request reservation; the bytes written
  ;; there are never read back.
  (func (export "cabi_realloc") (param i32 i32 i32 i32) (result i32) (i32.const 1024))

  (func (export "kobako:abi/guest@0.6.0#downcall") (param i32 i32) (result i32)
    (i32.const 992)))
