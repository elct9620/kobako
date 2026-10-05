;; Minimal wasm32 module that compiles and links but traps in its start
;; function, so the engine cannot instantiate it. The ABI surface is
;; otherwise complete, so instantiation is the step that refuses it.
(module
  (func $refuse unreachable)
  (start $refuse)
  (func (export "__kobako_eval"))
  (func (export "__kobako_run") (param i32 i32))
  (func (export "__kobako_abi_version") (result i32) (i32.const 3)))
