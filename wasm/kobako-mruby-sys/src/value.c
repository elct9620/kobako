/*
 * value.c — float unbox shim for the wasm32 word-box config.
 *
 * Why one shim survives
 * ---------------------
 * Under the kobako mruby configuration (`MRB_INT32` +
 * `MRB_WORDBOX_NO_INLINE_FLOAT`) mruby exposes no `MRB_API` float
 * accessor — the `mrb_float(v)` macro composes
 * `mrb_rfloat_value(mrb_val_union(v).fp)`. `mrb_val_union` returns
 * a `union mrb_value_` whose FFI return-value ABI differs between
 * bindgen's static-fn trampoline and rustc on wasm32; folding the
 * macro call into a single C translation unit sidesteps the
 * mismatch.
 *
 * Every other mrb_value helper this file used to wrap
 * (`kobako_value_is_integer`, `kobako_value_is_float`,
 * `kobako_unbox_integer`, `kobako_class_value`, the
 * `kobako_{nil,true,false}_value` immediates) has migrated to a
 * direct bindgen-routed call against mruby's own helpers — see the
 * call sites in `src/{value,class}.rs`.
 */

#include "mruby.h"
#include "mruby/value.h"
#include <stdint.h>

/* ─── Direct float unbox (precondition: caller has confirmed Float) ── */

MRB_API mrb_float
kobako_unbox_float(mrb_value v)
{
  return mrb_float(v);
}
