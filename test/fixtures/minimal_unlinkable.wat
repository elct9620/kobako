;; Minimal wasm32 module that compiles but imports a function no host
;; provides, so the engine cannot link it. The ABI surface is otherwise
;; complete, so the link step is the one that refuses it.
(module
  (import "env" "kobako_absent" (func))
  (func (export "kobako:abi/guest@0.6.0#downcall") (param i32 i32) (result i32) (i32.const 0)))
