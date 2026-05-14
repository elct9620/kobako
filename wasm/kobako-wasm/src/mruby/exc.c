/*
 * exc.c — layout-safe mrb->exc accessor shim.
 *
 * Purpose
 * -------
 * Rust cannot safely compute the byte offset of `mrb->exc` inside `mrb_state`
 * without mirroring the C struct — a fragile coupling that breaks silently if
 * mruby adds or reorders fields before `exc`. This shim delegates that
 * knowledge to mruby's own headers, which always reflect the correct layout
 * for the compiler and configuration in use.
 *
 * API
 * ---
 *   mrb_value kobako_get_exc(mrb_state *mrb)
 *
 * Returns `mrb_obj_value(mrb->exc)` if an exception is pending, or
 * `mrb_nil_value()` if there is no pending exception. Does NOT clear the
 * exception — callers must call `mrb_check_error` (or `mrb_clear_error`)
 * after consuming the returned value.
 *
 * Why the read/clear split is deliberate
 * --------------------------------------
 * `mrb->exc` is the only GC root for the pending exception object during
 * inspection. The returned `mrb_value` lives on the Rust stack, which
 * mruby's collector cannot scan — so the exception is rooted solely via
 * `mrb->exc` until the caller explicitly clears it. Callers typically need
 * to invoke `.message` / `.backtrace` (both of which allocate and may
 * trigger GC) before the data is consumed; clearing `mrb->exc` first would
 * make the exception eligible for collection mid-extraction, leaving Rust
 * holding a dangling reference. This mirrors mruby's own `mrb_print_error`
 * idiom: read, inspect, then clear.
 *
 * Usage pattern in abi.rs
 * -----------------------
 *   1. Call `mrb_load_nstring(mrb, ...)`.
 *   2. Call `kobako_get_exc(mrb)` to retrieve the exception (if any).
 *   3. If the returned value is non-nil, extract class name + message via
 *      `mrb_obj_classname` / `mrb_funcall(..., "message", 0)`.
 *   4. Call `mrb_check_error(mrb)` to clear `mrb->exc`.
 */

#include "mruby.h"
#include "mruby/value.h"

MRB_API mrb_value
kobako_get_exc(mrb_state *mrb)
{
  if (mrb->exc) {
    return mrb_obj_value(mrb->exc);
  }
  return mrb_nil_value();
}
