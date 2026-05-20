/*
 * bytecode.c — `#preload(binary:)` snippet loader with deferred structural
 * validation (docs/behavior.md B-32 binary: form, E-37 / E-38 / E-39).
 *
 * Purpose
 * -------
 * mruby's `mrb_load_irep_buf` couples parse + execute, returning only the
 * executed value. The bytecode preload path needs three things between
 * those two steps:
 *
 *   1. Distinguish "RITE header / IREP body parse failed" (E-37 / E-38)
 *      from "IREP loaded but its top-level execution raised" — the host
 *      gem attributes the former as `Kobako::BytecodeError` and the
 *      latter as the existing replay failure path.
 *   2. Inspect the loaded IREP's `debug_info` pointer to detect bytecode
 *      compiled without `mrbc -g` (E-39) — `mrb_load_irep_buf` accepts
 *      such blobs silently and the missing debug section only surfaces
 *      later as empty `Exception#backtrace`s, which is too late.
 *   3. Keep the IREP layout knowledge here: `struct mrb_irep` lives in
 *      `mruby/irep.h` and any field-offset reasoning has to follow that
 *      header rather than mirror it in Rust.
 *
 * API
 * ---
 *   int kobako_load_bytecode(mrb_state *mrb, const void *buf, size_t size)
 *
 * Returns 0 when the bytecode parsed, validated, and executed; non-zero
 * on any failure that left `mrb->exc` set. The caller treats every
 * non-zero return — and every case where `mrb->exc` is set on return —
 * as bytecode-form replay failure and reshapes the resulting Panic
 * envelope's `class` field to `Kobako::BytecodeError`.
 *
 * The function never longjmps; failures set `mrb->exc` and return so
 * the caller's `take_pending_panic` flow stays uniform with the source
 * snippet path.
 */

#include <string.h>

#include "mruby.h"
#include "mruby/dump.h"
#include "mruby/irep.h"
#include "mruby/proc.h"
#include "mruby/string.h"
#include "mruby/value.h"
#include "mruby/variable.h"

/* Synthesize an mruby exception under `mrb->exc` with the given fixed
 * diagnostic. `mrb_read_irep_buf` returns NULL silently on structural
 * failure (and `mrb_load_irep_buf` mirrors that by returning `undef`)
 * so the bytecode-load path has to set the exception itself before
 * returning to the caller's `take_pending_panic` flow. */
static void
set_bytecode_exc(mrb_state *mrb, const char *msg)
{
  mrb_value err = mrb_exc_new(mrb,
                              mrb_class_get(mrb, "RuntimeError"),
                              msg,
                              strlen(msg));
  mrb->exc = mrb_obj_ptr(err);
}

/* Pick a more specific diagnostic when `mrb_read_irep_buf` returns
 * NULL. The RITE binary header layout is fixed (mruby/dump.h: RITE
 * ident in bytes 0-3, format version in bytes 4-7) so we can split
 * E-37 (version mismatch) from E-38 (corrupt body or non-RITE input)
 * with a cheap memcmp pair. */
static const char *
classify_structural_failure(const void *buf, size_t size)
{
  if (size < sizeof(struct rite_binary_header)) {
    return "bytecode shorter than RITE binary header";
  }
  const char *bytes = (const char *)buf;
  if (memcmp(bytes, RITE_BINARY_IDENT, 4) != 0) {
    return "bytecode header is not RITE format";
  }
  if (memcmp(bytes + 4, RITE_BINARY_FORMAT_VER, 4) != 0) {
    return "bytecode RITE version mismatch";
  }
  return "bytecode body failed structural validation";
}

int
kobako_load_bytecode(mrb_state *mrb, const void *buf, size_t size)
{
  mrb_irep *irep = mrb_read_irep_buf(mrb, buf, size);
  if (irep == NULL) {
    /* E-37 (version) or E-38 (corrupt body / non-RITE input). The
     * caller's class-override step folds the synthesized exception
     * into BytecodeError. */
    set_bytecode_exc(mrb, classify_structural_failure(buf, size));
    return 1;
  }
  if (irep->debug_info == NULL) {
    /* E-39 — bytecode loaded structurally but carries no debug_info
     * section, so the snippet has no filename for backtrace attribution. */
    mrb_irep_decref(mrb, irep);
    set_bytecode_exc(mrb, "bytecode missing debug_info section");
    return 2;
  }
  /* Mirror the body of mruby's static `load_irep`: wrap the IREP in a
   * top-level Proc, hand IREP ownership to the Proc via decref, then
   * run. Any top-level raise sets mrb->exc and the caller's existing
   * path picks it up. */
  struct RProc *proc = mrb_proc_new(mrb, irep);
  proc->c = NULL;
  mrb_irep_decref(mrb, irep);
  mrb_top_run(mrb, proc, mrb_top_self(mrb), 0);
  return 0;
}
