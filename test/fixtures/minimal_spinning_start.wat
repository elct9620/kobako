;; Minimal wasm32 module whose start function never returns, so even the
;; construction-time ABI version check cannot finish instantiating it.
;; The ABI surface is otherwise complete, so instantiation is the step
;; that stalls.
(module
  (func $spin (loop $forever (br $forever)))
  (start $spin)
  (func (export "kobako:abi/guest@0.6.0#downcall") (param i32 i32) (result i32) (i32.const 0)))
