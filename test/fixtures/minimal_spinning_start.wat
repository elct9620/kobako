;; Minimal wasm32 module whose start function never returns, so even the
;; construction-time ABI version check cannot finish instantiating it.
;; The ABI surface is otherwise complete, so instantiation is the step
;; that stalls.
(module
  (func $spin (loop $forever (br $forever)))
  (start $spin)
  (func (export "__kobako_downcall") (param i32 i32) (result i64) (i64.const 0))
  (func (export "__kobako_abi_version") (result i32) (i32.const 5)))
