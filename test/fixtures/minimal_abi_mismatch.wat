;; `minimal_abi_ok.wat` with the reported ABI version replaced by 9999
;; — a value that must never match a real ABI version, keeping the
;; mismatch branch deterministic regardless of future version bumps
;; (same convention as `snippet_wrong_version.mrb`).
(module
  (func (export "__kobako_downcall") (param i32 i32) (result i64) (i64.const 0))
  (func (export "__kobako_abi_version") (result i32) (i32.const 9999)))
