/*
 * wrapper.h — bindgen entry point for the kobako-mruby-sys crate.
 *
 * Pulled in by `build.rs::run_bindgen` to expose the mruby C API
 * subset the kobako Guest Binary needs, plus the layout-safe C shims
 * compiled alongside mruby (see `src/{bytecode,exc,io,value}.c`).
 *
 * The shim forward declarations below are retired stage by stage as
 * each kobako_* helper is replaced by a direct call to mruby's own
 * helper — see `tmp/bindgen_migration_notes.md` for the staging plan.
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

/* Transitional shims — staged removal in follow-up commits. */
mrb_bool kobako_value_is_integer(mrb_value v);
mrb_bool kobako_value_is_float(mrb_value v);
mrb_int kobako_unbox_integer(mrb_value v);
mrb_float kobako_unbox_float(mrb_value v);
mrb_value kobako_nil_value(void);
mrb_value kobako_true_value(void);
mrb_value kobako_false_value(void);
mrb_value kobako_class_value(struct RClass *c);
mrb_value kobako_get_exc(mrb_state *mrb);
