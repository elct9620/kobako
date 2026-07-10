//! `__kobako_eval` — one-shot source invocation entry.
//!
//! Reactor entry that runs three jobs in sequence:
//!
//! 1. Read Frame 1 → install preamble groups; read Frame 2 (user
//!    script); read Frame 3 → replay snippets (docs/wire-codec.md
//!    § Invocation channels).
//! 2. Evaluate the user script under a `(eval)` ccontext so its IREP
//!    carries `debug_info` (needed for a populated
//!    `Exception#backtrace`).
//! 3. Serialize the last-expression value as a Result envelope, or
//!    convert the pending mruby exception into a Panic envelope, and
//!    write the bytes into the kobako-core outcome buffer.
//!
//! `__kobako_eval` never traps or calls `exit` — the host reads the
//! outcome tag from `__kobako_take_outcome()` after this function
//! returns.

/// Invocation entry behind the `__kobako_eval` export — see module
/// docs. `G` supplies the shell-chosen gem set via
/// `MrbGuest::init_gems`.
pub(crate) fn eval<G: crate::MrbGuest>() {
    eval_body::<G>();
}

fn eval_body<G: crate::MrbGuest>() {
    use super::boot;
    use super::mrb_slot::MRB;
    use beni::Ccontext;
    use kobako_codec::codec::Encode;
    use kobako_codec::outcome::Outcome;
    use kobako_core::abi::{write_outcome, write_panic};
    use kobako_core::frames;

    let preamble = match boot::read_preamble() {
        Ok(p) => p,
        Err(panic) => return write_panic(panic),
    };

    let frame2 = match frames::read_frame() {
        Some(b) => b,
        None => return write_panic(boot::boot_panic("failed to read the script")),
    };

    let snippets = match boot::read_snippets() {
        Ok(s) => s,
        Err(panic) => return write_panic(panic),
    };

    let kobako = match boot::acquire_vm::<G>() {
        Ok(k) => k,
        Err(panic) => return write_panic(panic),
    };
    let mrb = MRB.as_ref().expect("MRB live after acquire_vm");

    if let Err(panic) = boot::install_preamble(&kobako, &preamble) {
        return write_panic(panic);
    }

    if let Err(panic) = boot::replay_snippets(mrb, &kobako, &snippets) {
        return write_panic(panic);
    }

    // Compile under a ccontext with filename so the resulting IREP
    // carries `debug_info`; `pack_backtrace` in
    // `vendor/mruby/src/backtrace.c` skips any frame whose IREP has no
    // debug_info, which is why `Exception#backtrace` returns an empty
    // array when scripts are loaded via the bare `mrb_load_nstring`.
    let result_val = {
        let Some(cxt) = Ccontext::new(mrb, c"(eval)") else {
            return write_panic(boot::boot_panic(
                "failed to initialize the Sandbox interpreter",
            ));
        };
        cxt.load_nstring(&frame2)
        // `cxt` drops here — `mrb_ccontext_free` runs automatically.
    };

    if let Some(panic) = boot::take_pending_panic(mrb, &kobako) {
        write_panic(panic);
        return;
    }

    let Some(codec_value) = kobako.try_codec_value(result_val) else {
        return write_panic(boot::unrepresentable_return_panic(&kobako, result_val));
    };
    match Outcome::Value(codec_value).encode() {
        Ok(bytes) => write_outcome(bytes),
        Err(_) => write_panic(boot::transport_panic("result envelope encode failed")),
    }
    // The VM stays in the slot — the host discards the whole instance
    // after draining the outcome (ABI v2 per-invocation discipline).
}
