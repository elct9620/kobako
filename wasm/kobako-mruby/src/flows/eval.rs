//! `__kobako_eval` — one-shot source invocation entry.
//!
//! Reactor entry that runs three jobs in sequence:
//!
//! 1. Read Frame 1 → install preamble groups; read Frame 2 (user
//!    script); read Frame 3 → replay snippets (docs/wire/abi.md
//!    § Invocation channels).
//! 2. Evaluate the user script under a `(eval)` ccontext so its IREP
//!    carries `debug_info` (needed for a populated
//!    `Exception#backtrace`).
//! 3. Serialize the last-expression value as an ok Outcome, or
//!    convert the parse failure or raised exception into a Panic
//!    Outcome, and write the bytes into the kobako-core outcome buffer.
//!
//! `__kobako_eval` never traps or calls `exit` — the host reads the
//! outcome tag from `__kobako_take_outcome()` after this function
//! returns.

pub(crate) fn eval<G: crate::MrbGuest>() {
    use super::{boot, panic};
    use beni::Ccontext;
    use kobako_core::abi::write_panic;
    use kobako_core::frames;

    let preamble = match boot::read_preamble() {
        Ok(p) => p,
        Err(panic) => return write_panic(panic),
    };

    let frame2 = match frames::read_frame() {
        Some(b) => b,
        None => return write_panic(panic::boot_panic("failed to read the script")),
    };

    let snippets = match boot::read_snippets() {
        Ok(s) => s,
        Err(panic) => return write_panic(panic),
    };

    let kobako = match boot::enter::<G>(&preamble, &snippets) {
        Ok(k) => k,
        Err(panic) => return write_panic(panic),
    };
    let mrb = kobako.mrb();

    // Compile under a ccontext with filename so the resulting IREP
    // carries `debug_info`; `pack_backtrace` in
    // `vendor/mruby/src/backtrace.c` skips any frame whose IREP has no
    // debug_info, which is why `Exception#backtrace` returns an empty
    // array when scripts are loaded via the bare `mrb_load_nstring`.
    let filename = c"(eval)";
    let result = {
        let Some(cxt) = Ccontext::new(mrb, filename) else {
            return write_panic(panic::boot_panic(
                "failed to initialize the Sandbox interpreter",
            ));
        };
        cxt.load_nstring(&frame2)
        // `cxt` drops here — `mrb_ccontext_free` runs automatically.
    };

    match result {
        Ok(value) => panic::write_value_outcome::<G>(&kobako, value),
        Err(err) => write_panic(panic::load_panic(&kobako, filename, err)),
    }
    // The VM stays in the slot — the host discards the whole instance
    // after draining the outcome (the per-invocation discipline).
}
