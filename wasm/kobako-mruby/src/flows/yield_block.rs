//! The `yield` downcall — host-initiated re-entry into a guest block
//! (docs/wire/abi.md § ABI Signatures).
//!
//! The host sends it from inside an upcall it is still answering when
//! a Service method invokes its Yielder. The flow answers with a value;
//! `export_guest!` frames it into a buffer the host reads.
//!
//! ## Body
//!
//! 1. Resolve the active `mrb_state` via the module-level `MRB` slot and
//!    read the block of the open Call the request names off `BLOCK_STACK`.
//! 2. Read the yield arguments out of the request buffer through the
//!    guest's payload codec, which needs that VM to build values.
//! 3. Yield to the block through beni's protected `Proc::call` so any
//!    guest-side raise (or `break` / Proc-`return` RBreak) lands as `Err`
//!    instead of long-jumping past the Rust frame.
//! 4. Answer with the outcome as a Yield Reply:
//!     * normal return of a representable value → the ok arm carrying the
//!       value as a codec-encoded payload
//!     * a `break` or a Proc `return` leaving the block → the bare break
//!       arm, the exit held until the Service it unwinds answers
//!     * a raised exception, or a return value the codec has no
//!       representation for → the error arm carrying an Error Record

use kobako_transport::envelope::{ErrorRecord, YieldReply};

pub(crate) fn yield_to_block<G: crate::MrbGuest>(frame: u32, req: &[u8]) -> YieldReply {
    use super::mrb_slot::MRB;
    use crate::codec::PayloadCodec;
    use crate::runtime::block_stack::BLOCK_STACK;
    use crate::runtime::Kobako;

    // Step 1: resolve the active VM + Kobako runtime + bound block. The
    // codec needs the VM to build values, so the buffer is read after it.
    let Some(mrb) = MRB.as_ref() else {
        return error_reply(
            "RuntimeError",
            "block was called outside an active Sandbox invocation",
            Vec::new(),
        );
    };
    // SAFETY: MRB is `Some` only after `Kobako::init` ran for the
    // current invocation, satisfying `resolve_raw`'s precondition; the
    // active VM behind `mrb` outlives the returned token.
    let kobako = unsafe { Kobako::resolve_raw(mrb) };
    // Re-entering a block means the Service continued past whatever the
    // last one raised, which is what rescuing it means. Cleared before the
    // arguments are read so a refusal there — which holds no exception of
    // its own — cannot be answered with the spent one.
    crate::runtime::block_exit::BLOCK_RAISE.clear(mrb);
    let Some(block) = BLOCK_STACK.get(frame) else {
        return error_reply("LocalJumpError", "no block given (yield)", Vec::new());
    };

    // Step 2: read the yield arguments through the guest's codec. One it
    // cannot read fails the round-trip rather than reaching the block.
    let args = match G::Codec::decode_yield_arguments(&kobako, req) {
        Ok(args) => args,
        Err(err) => {
            let refusal = crate::refusal::at(crate::refusal::Position::YieldArguments, err);
            return error_reply(refusal.class, &refusal.message, Vec::new());
        }
    };

    // Step 3: protected yield via beni's `Proc::call`, which folds the
    // `mrb_yield_argv` + protect machinery — a guest-side raise / break /
    // Proc-`return` surfaces as `Err` instead of long-jumping past the
    // Rust frame.
    let result = block.call(mrb, args.as_slice());

    // Step 4: classify the outcome. Hold any exit object immediately on
    // the Err path, before any other mruby allocation could sweep it out
    // of the GC arena.
    match result {
        Ok(value) => value_reply::<G>(&kobako, value),
        Err(beni::Error::Exception(exc)) => exit_reply(&kobako, exc),
        // Calling a block compiles nothing, so no parse failure reaches
        // here, and a Rust panic inside the protected yield surfaces
        // only under unwinding panics — the guest builds with
        // `panic = "abort"`. Neither arm is reachable in production.
        Err(beni::Error::Syntax(_) | beni::Error::Panic(_)) => std::process::abort(),
    }
}

/// mruby already raises `E_LOCALJUMP_ERROR` for the orphan-block and
/// orphan-Proc shapes, so an RBreak here is a `break` or a Proc `return`
/// whose target is still on the guest stack, past this yield. The guest
/// keeps it and the Service only unwinds; the exit continues when that
/// Service's Call answers.
fn exit_reply(kobako: &crate::runtime::Kobako, exc: beni::Value) -> YieldReply {
    use beni::ReprValue;
    if exc.as_break().is_none() {
        return exception_reply(kobako, exc);
    }
    crate::runtime::block_exit::BLOCK_EXIT.set(kobako.mrb(), exc);
    YieldReply::Break
}

/// A value the schema cannot write surfaces as an error arm the host
/// Yielder reifies at the Service's yield site, rather than being coerced
/// to a String.
fn value_reply<G: crate::MrbGuest>(
    kobako: &crate::runtime::Kobako,
    value: beni::Value,
) -> YieldReply {
    use crate::codec::PayloadCodec;
    match G::Codec::encode_value(kobako, value) {
        Ok(payload) => YieldReply::Ok(payload),
        Err(err) => {
            let refusal = crate::refusal::at(crate::refusal::Position::BlockReturnValue, err);
            error_reply(refusal.class, &refusal.message, Vec::new())
        }
    }
}

fn exception_reply(kobako: &crate::runtime::Kobako, exc: beni::Value) -> YieldReply {
    let (class, message, backtrace) = super::panic::exception_fields(kobako, exc);
    // Held for the rest of the round-trip: the Service may rescue this,
    // and if it does not, its dispatch re-raises this very object rather
    // than a class and message rebuilt from the wire.
    crate::runtime::block_exit::BLOCK_RAISE.set(kobako.mrb(), exc);
    error_reply(&class, &message, backtrace)
}

fn error_reply(class: &str, message: &str, backtrace: Vec<String>) -> YieldReply {
    YieldReply::Error(ErrorRecord {
        name: class.into(),
        message: message.into(),
        backtrace,
    })
}
