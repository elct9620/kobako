;; Minimal wasm32 module that compiles but imports a function no host
;; provides, so the engine cannot link it. The ABI surface is otherwise
;; complete, so the link step is the one that refuses it.
(module
  (import "env" "kobako_absent" (func))
  (func (export "__kobako_downcall") (param i32 i32) (result i64) (i64.const 0))
  (func (export "__kobako_abi_version") (result i32) (i32.const 4)))
