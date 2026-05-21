/*
 * wrapper.h — bindgen entry point for the kobako-mruby-sys crate.
 *
 * Pulled in by `build.rs::run_bindgen` to expose the mruby C API the
 * kobako Guest Binary needs, plus the layout-safe C shim compiled
 * alongside mruby (see `src/bytecode.c`).
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

/* C shims that remain in this crate. */
int kobako_load_bytecode(mrb_state *mrb, const void *buf, size_t size);

/*
 * Static inline wrappers around mruby macros that lack a public
 * MRB_API / MRB_INLINE counterpart. bindgen's `wrap_static_fns`
 * picks these up and emits real extern symbols Rust can call, so
 * macro expansion stays inside the C compiler (which knows the
 * per-build word-box / string layout) rather than being mirrored
 * in Rust.
 */

/* Raw byte pointer into a String-tagged mrb_value. Counterpart to
 * the `RSTRING_PTR(s)` macro from <mruby/string.h>; the macro
 * branches between the embed buffer and the heap pointer based on
 * the RString header flags, which bindgen cannot read directly. */
static inline const char *
mrb_rstring_ptr(mrb_value s)
{
  return (const char *)RSTRING_PTR(s);
}

/* Byte length of a String-tagged mrb_value. Counterpart to
 * `RSTRING_LEN(s)`; same embed-vs-heap branch as `mrb_rstring_ptr`. */
static inline mrb_int
mrb_rstring_len(mrb_value s)
{
  return RSTRING_LEN(s);
}
