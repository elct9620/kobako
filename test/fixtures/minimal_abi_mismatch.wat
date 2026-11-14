;; `minimal_abi_ok.wat` with its downcall exported under ABI version 9999.0.0
;; — a value that must never match a real ABI version, keeping the
;; mismatch branch deterministic regardless of future version bumps
;; (same convention as `snippet_wrong_version.mrb`).
(module
  (func (export "kobako:abi/guest@9999.0.0#downcall") (param i32 i32) (result i32) (i32.const 0)))
