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
//!    write the bytes into [`super::outcome_buffer`].
//!
//! `__kobako_eval` never traps or calls `exit` — the host reads the
//! outcome tag from `__kobako_take_outcome()` after this function
//! returns.

/// Reactor entry — see module docs.
#[no_mangle]
pub extern "C" fn __kobako_eval() {
    #[cfg(target_arch = "wasm32")]
    {
        eval_body();
    }
}

#[cfg(target_arch = "wasm32")]
fn eval_body() {
    use super::boot;
    use super::frames;
    use super::outcome_buffer::{write_outcome, write_panic};
    use crate::mruby::Ccontext;
    use crate::outcome::{encode_outcome, Outcome, Panic};

    let preamble = match boot::read_preamble() {
        Ok(p) => p,
        Err(panic) => return write_panic(panic),
    };

    let frame2 = match frames::read_frame() {
        Some(b) => b,
        None => return write_panic(boot::boot_panic("failed to read script frame")),
    };

    let snippets = match boot::read_snippets() {
        Ok(s) => s,
        Err(panic) => return write_panic(panic),
    };

    let (mrb, kobako) = match boot::open_with_preamble(&preamble) {
        Ok(pair) => pair,
        Err(panic) => return write_panic(panic),
    };

    if let Err(panic) = boot::replay_snippets(&mrb, &kobako, &snippets) {
        return write_panic(panic);
    }

    // Compile under a ccontext with filename so the resulting IREP
    // carries `debug_info`; `pack_backtrace` in
    // `vendor/mruby/src/backtrace.c` skips any frame whose IREP has no
    // debug_info, which is why `Exception#backtrace` returns an empty
    // array when scripts are loaded via the bare `mrb_load_nstring`.
    let result_val = {
        let Some(cxt) = Ccontext::new(&mrb, c"(eval)") else {
            return write_panic(boot::boot_panic("mrb_ccontext_new returned NULL"));
        };
        cxt.load_nstring(&frame2)
        // `cxt` drops here — `mrb_ccontext_free` runs automatically.
    };

    if let Some(panic) = boot::take_pending_panic(&mrb, &kobako) {
        write_panic(panic);
        return;
    }

    let wire_value = kobako.to_wire_outcome(result_val);
    match encode_outcome(&Outcome::Value(wire_value)) {
        Ok(bytes) => write_outcome(bytes),
        Err(_) => write_panic(Panic {
            origin: "sandbox".into(),
            class: "Kobako::RPC::WireError".into(),
            message: "result envelope encode failed".into(),
            backtrace: Vec::new(),
            details: None,
        }),
    }
    // `mrb` drops here — `mrb_close` runs automatically.
}
