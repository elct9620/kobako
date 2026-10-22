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
//!     * a real `break` from a non-lambda block → the break arm
//!     * a raised exception, a return value the codec has no
//!       representation for, or an RBreak aimed past the yielder's frame
//!       (a non-orphan Proc `return`) → the error arm carrying an Error
//!       Record

use kobako_transport::envelope::{ErrorRecord, YieldReply};

pub(crate) fn yield_to_block<G: crate::MrbGuest>(frame: u32, req: &[u8]) -> YieldReply {
    use super::mrb_slot::MRB;
    use crate::codec::PayloadCodec;
    use crate::runtime::block_stack::BLOCK_STACK;
    use crate::runtime::Kobako;
    use beni::sys;

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
    crate::runtime::raised_block::RAISED_BLOCK.clear(mrb);
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
    // Rust frame. Snapshot the current callinfo index *before* the call
    // so step 4's classification can place any RBreak destination
    // relative to this yielder's frame.
    // SAFETY: `mrb` is live by the outer `&Mrb` borrow; the shim reads
    // the VM-internal `mrb_context.ci` / `cibase` frame indices, which
    // carry no MRB_API accessor and so stay on the unsafe `sys` seam.
    let enter_idx = unsafe { sys::mrb_current_ci_index_func(mrb.as_ptr()) };
    let result = block.call(mrb, args.as_slice());

    // Step 4: classify the outcome. Extract any exception fields
    // immediately on the Err path before any other mruby allocation
    // could sweep the exception object out of the GC arena. RBreak
    // outcomes split on `ci_break_index` vs `enter_idx`.
    match result {
        Ok(value) => value_reply::<G>(
            &kobako,
            value,
            YieldReply::Ok,
            crate::refusal::Position::BlockReturnValue,
        ),
        Err(beni::Error::Exception(exc)) => classify_protected_error::<G>(&kobako, exc, enter_idx),
        // Calling a block compiles nothing, so no parse failure reaches
        // here, and a Rust panic inside the protected yield surfaces
        // only under unwinding panics — the guest builds with
        // `panic = "abort"`. Neither arm is reachable in production.
        Err(beni::Error::Syntax(_) | beni::Error::Panic(_)) => std::process::abort(),
    }
}

/// mruby already raises `E_LOCALJUMP_ERROR` for the orphan-block and
/// orphan-Proc shapes, so an RBreak here is either a real `break` or a
/// non-orphan Proc `return`; `RBreak.ci_break_index` against `enter_idx`
/// tells them apart.
fn classify_protected_error<G: crate::MrbGuest>(
    kobako: &crate::runtime::Kobako,
    exc: beni::Value,
    enter_idx: usize,
) -> YieldReply {
    use beni::sys;
    use beni::sys::AsRawValue;
    use beni::ReprValue;
    // A non-break exception is a plain raise — the error arm.
    let Some(brk) = exc.as_break() else {
        return exception_reply(kobako, exc);
    };
    // SAFETY: `exc` is RBreak-tagged (`as_break` returned `Some`); the
    // shim reads `RBreak.ci_break_index`, a VM-internal field with no
    // MRB_API accessor, so it stays on the unsafe `sys` seam.
    let brk_idx = unsafe { sys::mrb_break_ci_index_func(exc.as_raw()) };
    if brk_idx >= enter_idx {
        value_reply::<G>(
            kobako,
            brk.value(),
            YieldReply::Break,
            crate::refusal::Position::BreakValue,
        )
    } else {
        // RBreak whose destination is deeper than the yielder's frame —
        // a non-orphan Proc `return` aimed at an outer guest method, or a
        // `break` from a block run inside a later Call — would unwind
        // across a host yield boundary, which the wire cannot carry.
        error_reply(
            "LocalJumpError",
            "cannot break or return from a block past the Service that runs it",
            Vec::new(),
        )
    }
}

/// A value the schema cannot write surfaces as an error arm the host
/// Yielder reifies at the Service's yield site, rather than being coerced
/// to a String.
fn value_reply<G: crate::MrbGuest>(
    kobako: &crate::runtime::Kobako,
    value: beni::Value,
    arm: fn(Vec<u8>) -> YieldReply,
    position: crate::refusal::Position,
) -> YieldReply {
    use crate::codec::PayloadCodec;
    match G::Codec::encode_value(kobako, value) {
        Ok(payload) => arm(payload),
        Err(err) => {
            let refusal = crate::refusal::at(position, err);
            error_reply(refusal.class, &refusal.message, Vec::new())
        }
    }
}

fn exception_reply(kobako: &crate::runtime::Kobako, exc: beni::Value) -> YieldReply {
    let (class, message, backtrace) = super::panic::exception_fields(kobako, exc);
    // Held for the rest of the round-trip: the Service may rescue this,
    // and if it does not, its dispatch re-raises this very object rather
    // than a class and message rebuilt from the wire.
    crate::runtime::raised_block::RAISED_BLOCK.set(kobako.mrb(), exc);
    error_reply(&class, &message, backtrace)
}

fn error_reply(class: &str, message: &str, backtrace: Vec<String>) -> YieldReply {
    YieldReply::Error(ErrorRecord {
        name: class.into(),
        message: message.into(),
        backtrace,
    })
}
