;; A guest whose compile never finishes: it grows memory by 16 MiB, then
;; spins until a cap stops it. Every other downcall answers a nil Result at
;; once, so an invocation's own cost stays near zero beside the compile's.
;;
;; The version is the one in the export name; update it by hand on a bump,
;; same as `minimal_abi_ok.wat`.
(module
  (memory (export "memory") 1)

  ;; The nil Result: the registry's ok tag 0x10, then nil 0xc0.
  (data (i32.const 8) "\10\c0")

  ;; The return area at 992 naming the nil Result: place 8, length 2.
  (data (i32.const 992) "\08\00\00\00\02\00\00\00")

  ;; Any room satisfies the host's reservations; nothing written there is
  ;; read back once its tag is.
  (func (export "cabi_realloc") (param i32 i32 i32 i32) (result i32) (i32.const 1024))

  ;; The request's first byte is its tag: 0x08 compile.
  (func (export "kobako:abi/guest@0.6.0#downcall") (param $ptr i32) (param $len i32) (result i32)
    (if (i32.eq (i32.load8_u (local.get $ptr)) (i32.const 0x08))
      (then
        (drop (memory.grow (i32.const 256)))
        (loop $spin (br $spin))))
    (i32.const 992)))
