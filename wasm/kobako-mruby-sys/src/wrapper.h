/*
 * wrapper.h — bindgen entry point for the kobako-mruby-sys crate.
 *
 * Pulled in by `build.rs::run_bindgen` to expose the mruby C API
 * the kobako Guest Binary needs. No hand-written C translation
 * units live in the crate any more: the static inline wrappers
 * below are the entire C surface, and bindgen's `wrap_static_fns`
 * emits a single trampoline file from them.
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
#include <mruby/proc.h>
#include <mruby/string.h>
#include <mruby/value.h>
#include <mruby/variable.h>

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

/* Object pointer extractor from an object-tagged mrb_value.
 * Counterpart to the `mrb_obj_ptr(v)` macro in <mruby/value.h>,
 * which expands via `mrb_val_union(v).p`. Folding the union read
 * into a single C function sidesteps the wasm32 union-return ABI
 * mismatch bindgen's trampoline would otherwise hit. */
static inline struct RObject *
mrb_obj_ptr_func(mrb_value v)
{
  return mrb_obj_ptr(v);
}

/* GC arena bracketing helpers. mruby exposes these as macros that
 * read / write `mrb->gc.arena_idx`; bindgen treats `mrb_gc` as
 * opaque (workaround for the bitfield mis-pack on wasm32, see
 * `build.rs::run_bindgen`), so reaching the field from Rust
 * requires routing through the C compiler. */
static inline int
mrb_gc_arena_save_func(mrb_state *mrb)
{
  return mrb_gc_arena_save(mrb);
}

static inline void
mrb_gc_arena_restore_func(mrb_state *mrb, int idx)
{
  mrb_gc_arena_restore(mrb, idx);
}

/* `mrb_proc_new` is declared in <mruby/proc.h> without `MRB_API`,
 * so the `-fvisibility=default` workaround in `build.rs` does not
 * make bindgen pick it up. The static archive still resolves the
 * symbol at link time; wrap it in a `static inline` here so
 * bindgen's `wrap_static_fns` emits a trampoline Rust can call. */
static inline struct RProc *
mrb_proc_new_func(mrb_state *mrb, const mrb_irep *irep)
{
  return mrb_proc_new(mrb, irep);
}
