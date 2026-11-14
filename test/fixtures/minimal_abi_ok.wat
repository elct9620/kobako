;; Minimal wasm32 module that passes the construction-time ABI version
;; check while stubbing the downcall as
;; a no-op. Successor to `minimal.wasm` for tests that only need
;; `Kobako::Sandbox.new(wasm_path:)` to succeed; it has no memory or
;; allocator, so an invocation against it traps before reaching the guest.
;; Text format on purpose — the ext enables wasmtime's `wat` feature, so
;; the file loads through the same `wasm_path:` path as a binary artifact.
;; The version is the one in the export name; update it by hand on a bump.
(module
  (func (export "kobako:abi/guest@0.6.0#downcall") (param i32 i32) (result i32) (i32.const 0)))
