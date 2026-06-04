;; Minimal wasm32 module that passes the B-40 construction-time ABI
;; version check (docs/wire-codec.md § ABI Version) while stubbing the
;; two invocation entry points as no-ops. Successor to `minimal.wasm`
;; for tests that only need `Kobako::Sandbox.new(wasm_path:)` to
;; succeed; never invoked end-to-end. Text format on purpose — the ext
;; enables wasmtime's `wat` feature, so the file loads through the same
;; `wasm_path:` path as a binary artifact.
(module
  (func (export "__kobako_eval"))
  (func (export "__kobako_run") (param i32 i32))
  (func (export "__kobako_abi_version") (result i32) (i32.const 1)))
