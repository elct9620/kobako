;; Minimal wasm32 module that passes the construction-time ABI version
;; check (docs/wire/abi.md § ABI Version) while stubbing the downcall as
;; a no-op. Successor to `minimal.wasm` for tests that only need
;; `Kobako::Sandbox.new(wasm_path:)` to succeed; it has no memory or
;; allocator, so an invocation against it traps before reaching the guest.
;; Text format on purpose — the ext enables wasmtime's `wat` feature, so
;; the file loads through the same `wasm_path:` path as a binary artifact.
(module
  (func (export "__kobako_downcall") (param i32 i32) (result i64) (i64.const 0))
  (func (export "__kobako_abi_version") (result i32) (i32.const 4)))
