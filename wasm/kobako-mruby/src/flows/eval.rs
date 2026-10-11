//! The `eval` downcall — one-shot source invocation entry.
//!
//! Reactor entry that runs three jobs in sequence:
//!
//! 1. Apply the flags, install the bound paths' proxies, and replay the
//!    snippets.
//! 2. Evaluate the user script under a `(eval)` ccontext so its IREP
//!    carries `debug_info` (needed for a populated
//!    `Exception#backtrace`).
//! 3. Answer with the last-expression value as an ok Outcome, or
//!    convert the parse failure or raised exception into a Panic
//!    Outcome.
//!
//! This flow never traps or calls `exit` — every failure is an
//! Outcome the host reads after this function returns.

use kobako_transport::envelope::{Outcome, Panic, Preamble};

pub(crate) fn eval<G: crate::MrbGuest>(preamble: Preamble, source: Vec<u8>) -> Outcome {
    evaluate::<G>(&preamble, &source).unwrap_or_else(Outcome::Panic)
}

fn evaluate<G: crate::MrbGuest>(preamble: &Preamble, source: &[u8]) -> Result<Outcome, Panic> {
    use super::{boot, panic};
    use beni::Ccontext;

    let kobako = boot::enter::<G>(preamble)?;
    let mrb = kobako.mrb();

    // Compile under a ccontext with filename so the resulting IREP
    // carries `debug_info`; `pack_backtrace` in
    // `vendor/mruby/src/backtrace.c` skips any frame whose IREP has no
    // debug_info, which is why `Exception#backtrace` returns an empty
    // array when scripts are loaded via the bare `mrb_load_nstring`.
    let filename = c"(eval)";
    let result = {
        let cxt = Ccontext::new(mrb, filename)
            .ok_or_else(|| panic::boot_panic("failed to initialize the Sandbox interpreter"))?;
        cxt.load_nstring(source)
        // `cxt` drops here — `mrb_ccontext_free` runs automatically.
    };

    // The VM stays in the slot — the host discards the whole instance
    // after draining the outcome (the per-invocation discipline).
    match result {
        Ok(value) => Ok(panic::value_outcome::<G>(&kobako, value)),
        Err(err) => Err(panic::load_panic(&kobako, filename, err)),
    }
}
