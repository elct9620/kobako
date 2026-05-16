/*
 * io.c — C shim for `Kobako::IO#write` byte-pumping.
 *
 * Why C
 * -----
 * Pumping a mruby `String` value through `fwrite` requires three things
 * the Rust FFI cannot reach without per-build header offsets:
 *
 *   1. `RSTRING_PTR(s)` / `RSTRING_LEN(s)` — macros that read either
 *      embedded buffer fields or the heap-allocated overflow pointer,
 *      depending on the string's tagged layout. mruby's headers track
 *      the per-build choice; mirroring it from Rust would be a fragile
 *      ABI dependency.
 *   2. `mrb_obj_as_string(mrb, val)` — coerces any value to a String,
 *      respecting `Object#to_s` overrides. Public mruby API.
 *   3. wasi-libc `stdout` / `stderr` — `FILE *` globals that Rust
 *      `extern` declarations would have to match libc's exact ABI;
 *      doing it from C side-steps that.
 *
 * Contract
 * --------
 *   mrb_int kobako_io_fwrite(mrb_state *mrb, int fd,
 *                            const mrb_value *argv, mrb_int argc)
 *
 *   - `fd` selects the stream: 2 routes to stderr, anything else
 *     (canonically 1) routes to stdout. Validation against the
 *     {1, 2} allowlist happens host-side in `IO#initialize`; this
 *     shim trusts what it receives.
 *   - Each `argv[i]` is coerced via `mrb_obj_as_string` (may raise
 *     `TypeError` on objects whose `to_s` does not return a String).
 *   - Returns the total bytes written across all arguments.
 *
 * Truncation: when `fwrite` short-writes (e.g. WASI capture pipe hit
 * its cap, see SPEC.md B-04), the per-arg `total` reflects the actual
 * bytes accepted; the host observes the truncation via the
 * MemoryOutputPipe cap, not via a Ruby-level error from this shim.
 */

#include "mruby.h"
#include "mruby/string.h"
#include <stdio.h>

MRB_API mrb_int
kobako_io_fwrite(mrb_state *mrb, int fd, const mrb_value *argv, mrb_int argc)
{
  FILE *stream = (fd == 2) ? stderr : stdout;
  mrb_int total = 0;
  for (mrb_int i = 0; i < argc; i++) {
    mrb_value s = mrb_obj_as_string(mrb, argv[i]);
    mrb_int len = RSTRING_LEN(s);
    if (len > 0) {
      total += (mrb_int)fwrite(RSTRING_PTR(s), 1, (size_t)len, stream);
    }
  }
  return total;
}
