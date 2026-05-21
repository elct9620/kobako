/*
 * value.c — thin shim over mruby's macro-only mrb_value helpers.
 *
 * Purpose
 * -------
 * mruby exposes several mrb_value primitives as macros whose expansion
 * depends on the word-box configuration. Rust cannot reach them through
 * `extern "C"` without unresolved imports. Wrapping them in real
 * `MRB_API` functions delegates the boxing-aware bit twiddling to
 * mruby's own headers, so kobako never has to know whether the build
 * uses MRB_INT32 + MRB_WORDBOX_NO_INLINE_FLOAT or any other layout.
 *
 * What this shim does NOT do
 * --------------------------
 * No hand-rolled boxing knowledge. No hard-coded tag bits, no manual
 * `MRB_Qnil` / `MRB_Qtrue` / `MRB_Qfalse` word values. Everything is a
 * call into the mruby header — the header decides the bit layout.
 *
 * Direct unbox
 * ------------
 * Integer unboxing is no longer wrapped here: the safe layer calls
 * mruby's own `mrb_integer_func` (`MRB_INLINE`) directly through
 * bindgen's static-fn trampoline.
 *
 * Float unboxing keeps a shim because under
 * `MRB_WORDBOX_NO_INLINE_FLOAT` mruby exposes no `MRB_API` float
 * accessor — the `mrb_float(v)` macro composes
 * `mrb_rfloat_value(mrb_val_union(v).fp)`, and `mrb_val_union`
 * returns a `union mrb_value_` whose FFI return-value ABI differs
 * between bindgen's trampoline and rustc on wasm32. Keeping the
 * macro call inside C avoids that mismatch.
 */

#include "mruby.h"
#include "mruby/value.h"
#include "mruby/class.h"
#include <stdint.h>

/* ─── Type predicates ───────────────────────────────────────────── */

MRB_API mrb_bool
kobako_value_is_integer(mrb_value v)
{
  return mrb_integer_p(v);
}

MRB_API mrb_bool
kobako_value_is_float(mrb_value v)
{
  return mrb_float_p(v);
}

/* ─── Direct float unbox (precondition: caller has confirmed Float) ── */

MRB_API mrb_float
kobako_unbox_float(mrb_value v)
{
  return mrb_float(v);
}

/* ─── Word-box constant accessors ───────────────────────────────── */
/*
 * Wrap mruby's own `mrb_nil_value()` / `mrb_true_value()` /
 * `mrb_false_value()` macros so the kobako runtime does not have to
 * mirror the bit pattern in Rust. The Rust side caches the three
 * mrb_value results inside `Kobako` at install time and reuses them
 * for every value construction site.
 */

MRB_API mrb_value
kobako_nil_value(void)
{
  return mrb_nil_value();
}

MRB_API mrb_value
kobako_true_value(void)
{
  return mrb_true_value();
}

MRB_API mrb_value
kobako_false_value(void)
{
  return mrb_false_value();
}

/* ─── Class → mrb_value wrapping ─────────────────────────────────── */
/*
 * `mrb_obj_value(p)` is an inline function in `mruby/value.h` whose
 * expansion encodes the word-box object tag. Calling it from Rust
 * requires a real `MRB_API` thunk; `kobako_class_value(c)` packages a
 * `struct RClass *` (the form the kobako install paths cache) into the
 * `mrb_value` the `mrb_const_defined` / `mrb_const_get` family expects.
 */
MRB_API mrb_value
kobako_class_value(struct RClass *c)
{
  return mrb_obj_value(c);
}
