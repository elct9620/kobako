/*
 * wrapper.h — bindgen entry point for the kobako-mruby-sys crate.
 *
 * Pulled in by `build.rs::run_bindgen` to expose the mruby C API the
 * kobako Guest Binary needs, plus the layout-safe C shims compiled
 * alongside mruby (see `src/{bytecode,exc,io,value}.c`).
 */

#include <mruby.h>
#include <mruby/array.h>
#include <mruby/class.h>
#include <mruby/dump.h>
#include <mruby/error.h>
#include <mruby/hash.h>
#include <mruby/irep.h>
#include <mruby/string.h>
#include <mruby/value.h>
#include <mruby/variable.h>

/* C shims that remain in this crate after the full migration. */
mrb_int kobako_io_fwrite(mrb_state *mrb, int fd, const mrb_value *argv, mrb_int argc);
int kobako_load_bytecode(mrb_state *mrb, const void *buf, size_t size);

/* kobako_unbox_float — the wasm32 MRB_WORDBOX_NO_INLINE_FLOAT config
 * has no MRB_API float accessor; keeping the macro call inside C
 * avoids a union-return ABI mismatch on the bindgen path. */
mrb_float kobako_unbox_float(mrb_value v);

/* Transitional shims — staged removal in follow-up commits. */
mrb_bool kobako_value_is_integer(mrb_value v);
mrb_bool kobako_value_is_float(mrb_value v);
mrb_value kobako_get_exc(mrb_state *mrb);
