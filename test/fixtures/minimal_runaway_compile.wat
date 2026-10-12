;; A guest whose compile never finishes: it grows memory by 16 MiB, then
;; spins until a cap stops it. Every other downcall answers a nil Result at
;; once, so an invocation's own cost stays near zero beside the compile's.
;;
;; Update the `i32.const` ABI version by hand on a bump, same as
;; `minimal_abi_ok.wat`.
(module
  (memory (export "memory") 1)

  ;; The nil Result: the registry's ok tag 0x10, then nil 0xc0.
  (data (i32.const 8) "\10\c0")

  ;; Any non-zero offset satisfies the host's reservations; nothing written
  ;; there is read back once its tag is.
  (func (export "__kobako_alloc") (param i32) (result i32) (i32.const 1024))

  ;; The request's first byte is its tag: 0x08 compile.
  (func (export "__kobako_downcall") (param $ptr i32) (param $len i32) (result i64)
    (if (i32.eq (i32.load8_u (local.get $ptr)) (i32.const 0x08))
      (then
        (drop (memory.grow (i32.const 256)))
        (loop $spin (br $spin))))
    (i64.or (i64.shl (i64.const 8) (i64.const 32)) (i64.const 2)))

  (func (export "__kobako_abi_version") (result i32) (i32.const 4)))
