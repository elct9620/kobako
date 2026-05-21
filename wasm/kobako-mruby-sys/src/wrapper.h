/*
 * wrapper.h — bindgen entry point for the kobako-mruby-sys crate.
 *
 * Pulled in by `build.rs::run_bindgen` to expose the mruby C API the
 * kobako Guest Binary needs, plus the layout-safe C shims compiled
 * alongside mruby (see `src/{bytecode,io}.c`).
 *
 * The `<stdbool.h>` and `<sys/select.h>` pre-includes are not used
 * by the mruby surface itself — they cover bindgen's `wrap_static_fns`
 * trampoline file. bindgen emits a trampoline for every `static inline`
 * function reached through the include tree, including wasi-libc
 * helpers like `FD_ISSET`. The generated trampoline file `#include`s
 * only this wrapper, so `bool` and `fd_set` must resolve here even
 * though the safe layer never calls those helpers. Release builds
 * happened to inline-strip the unused trampolines; debug builds keep
 * them, which is what surfaces the compile failure without these
 * pre-includes.
 */

#include <stdbool.h>
#include <sys/select.h>

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
