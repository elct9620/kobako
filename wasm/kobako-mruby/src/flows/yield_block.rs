//! `__kobako_yield_to_block` — host-initiated re-entry into a guest
//! block (docs/wire-codec.md § ABI Signatures).
//!
//! The host calls this from inside a `__kobako_dispatch` callback when
//! a Service method invokes its Yielder. The signature mirrors
//! `__kobako_dispatch`'s — `(req_ptr, req_len) -> i64` with the same
//! packed-u64 `(ptr<<32)|len` return — so the same alloc / write /
//! read shape applies in the symmetric direction.
//!
//! ## Body
//!
//! 1. Resolve the active `mrb_state` via the module-level `MRB` slot and
//!    read the topmost block off `BLOCK_STACK`.
//! 2. Read the yield arguments out of the request buffer through the
//!    guest's payload codec, which needs that VM to build values.
//! 3. Yield to the block through beni's protected `Proc::call` so any
//!    guest-side raise (or `break` / Proc-`return` RBreak) lands as `Err`
//!    instead of long-jumping past the Rust frame.
//! 4. Encode the outcome as a `Yield Reply`:
//!     * normal return of a representable value → the ok arm carrying the
//!       value as a codec-encoded payload
//!     * a real `break` from a non-lambda block → the break arm
//!     * a raised exception, a return value the codec has no
//!       representation for, or an RBreak aimed past the yielder's frame
//!       (a non-orphan Proc `return`) → the error arm carrying an Error
//!       Record
//! 5. Allocate the response buffer via `__kobako_alloc`, copy the
//!    bytes in, return the packed `(ptr<<32)|len`.

#[cfg(mruby_linked)]
use kobako_core::abi::pack_ptr_len;
#[cfg(mruby_linked)]
use kobako_transport::envelope::{ErrorRecord, YieldReply};

/// Invocation entry behind the `__kobako_yield_to_block` export —
/// see module docs. Signature pinned by docs/wire-codec.md § ABI
/// Signatures (5 guest exports).
#[cfg(mruby_linked)]
pub(crate) fn yield_to_block<G: crate::MrbGuest>(req: &[u8]) -> u64 {
    yield_to_block_body::<G>(req)
}

#[cfg(mruby_linked)]
fn yield_to_block_body<G: crate::MrbGuest>(req: &[u8]) -> u64 {
    use super::mrb_slot::MRB;
    use crate::codec::PayloadCodec;
    use crate::runtime::block_stack::BLOCK_STACK;
    use crate::runtime::Kobako;
    use beni::{sys, FromValue, Proc};

    // Step 1: resolve the active VM + Kobako runtime + bound block. The
    // codec needs the VM to build values, so the buffer is read after it.
    let Some(mrb) = MRB.as_ref() else {
        return write_error_response(
            "RuntimeError",
            "block was called outside an active Sandbox invocation",
            Vec::new(),
        );
    };
    // SAFETY: MRB is `Some` only after `Kobako::init` ran for the
    // current invocation, satisfying `resolve_raw`'s precondition; the
    // active VM behind `mrb` outlives the returned token.
    let kobako = unsafe { Kobako::resolve_raw(mrb) };
    let Some(block) = BLOCK_STACK.last().and_then(Proc::from_value) else {
        return write_error_response("LocalJumpError", "no block given (yield)", Vec::new());
    };

    // Step 2: read the yield arguments through the guest's codec. One
    // the guest cannot represent — an integer outside the 32-bit range —
    // fails the round-trip rather than reaching the block with a saturated
    // value (docs/wire/payload-msgpack.md § Integer Range).
    let args = match G::Codec::decode_yield_arguments(&kobako, req) {
        Ok(args) => args,
        Err(err) => {
            let refusal = crate::refusal::at(crate::refusal::Position::YieldArguments, err);
            return write_error_response(refusal.class, refusal.message, Vec::new());
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
    let result = block.call(mrb, &args);

    // Step 4: encode the outcome. Extract any exception fields
    // immediately on the Err path before any other mruby allocation
    // could sweep the exception object out of the GC arena. RBreak
    // outcomes split on `ci_break_index` vs `enter_idx`.
    let bytes = match result {
        Ok(value) => encode_ok_response::<G>(&kobako, value),
        Err(beni::Error::Exception(exc)) => classify_protected_error::<G>(&kobako, exc, enter_idx),
        // A Rust panic inside the protected yield can only surface
        // here under unwinding panics; the guest builds with
        // `panic = "abort"`, so this arm is unreachable in production.
        Err(beni::Error::Panic(_)) => std::process::abort(),
    };
    write_yield_buffer(&bytes)
}

/// Classify the value the protected `Proc::call` surfaced on its `Err`
/// path into a Yield Reply. mruby's VM already raises
/// `E_LOCALJUMP_ERROR` directly for the orphan-block / orphan-Proc
/// shapes, so any RBreak we see here is either a
/// real `break` from a non-lambda block or a non-orphan Proc `return`
/// — discriminate them by comparing `RBreak.ci_break_index` against
/// the `enter_idx` snapshot taken immediately before the protected
/// yield.
#[cfg(mruby_linked)]
fn classify_protected_error<G: crate::MrbGuest>(
    kobako: &crate::runtime::Kobako,
    exc: beni::Value,
    enter_idx: usize,
) -> Vec<u8> {
    use beni::sys;
    // A non-break exception is a plain raise — tag 0x04.
    let Some(brk) = exc.as_break() else {
        return encode_error_response_from_exception(kobako, exc);
    };
    // SAFETY: `exc` is RBreak-tagged (`as_break` returned `Some`); the
    // shim reads `RBreak.ci_break_index`, a VM-internal field with no
    // MRB_API accessor, so it stays on the unsafe `sys` seam.
    let brk_idx = unsafe { sys::mrb_break_ci_index_func(exc.as_raw()) };
    if brk_idx >= enter_idx {
        encode_break_response::<G>(kobako, brk.value())
    } else {
        // RBreak whose destination is deeper than the yielder's frame
        // is a non-orphan Proc `return` aimed at an outer guest method
        // — unrepresentable across the host yield boundary.
        encode_error_bytes(
            "LocalJumpError",
            "cannot return from a block passed into the Sandbox",
            Vec::new(),
        )
    }
}

/// Encode a value-carrying Yield Reply (the ok or break arm). A value the
/// schema cannot write surfaces as an error arm the host Yielder reifies
/// at the Service's yield site, rather than being coerced to a String.
#[cfg(mruby_linked)]
fn encode_value_response<G: crate::MrbGuest>(
    kobako: &crate::runtime::Kobako,
    value: beni::Value,
    arm: fn(Vec<u8>) -> YieldReply,
    position: crate::refusal::Position,
) -> Vec<u8> {
    use crate::codec::PayloadCodec;
    match G::Codec::encode_value(kobako, value) {
        Ok(payload) => arm(payload).encode(),
        Err(err) => {
            let refusal = crate::refusal::at(position, err);
            encode_error_bytes(refusal.class, &refusal.message, Vec::new())
        }
    }
}

#[cfg(mruby_linked)]
fn encode_break_response<G: crate::MrbGuest>(
    kobako: &crate::runtime::Kobako,
    value: beni::Value,
) -> Vec<u8> {
    use crate::refusal::Position;
    encode_value_response::<G>(kobako, value, YieldReply::Break, Position::BreakValue)
}

#[cfg(mruby_linked)]
fn encode_ok_response<G: crate::MrbGuest>(
    kobako: &crate::runtime::Kobako,
    value: beni::Value,
) -> Vec<u8> {
    use crate::refusal::Position;
    encode_value_response::<G>(kobako, value, YieldReply::Ok, Position::BlockReturnValue)
}

#[cfg(mruby_linked)]
fn encode_error_response_from_exception(
    kobako: &crate::runtime::Kobako,
    exc: beni::Value,
) -> Vec<u8> {
    let (class, message, backtrace) = super::boot::exception_fields(kobako, exc);
    encode_error_bytes(&class, &message, backtrace)
}

#[cfg(mruby_linked)]
fn encode_error_bytes(class: &str, message: &str, backtrace: Vec<String>) -> Vec<u8> {
    YieldReply::Error(ErrorRecord {
        name: class.into(),
        message: message.into(),
        backtrace,
    })
    .encode()
}

/// Write an error Yield Reply directly into a fresh guest buffer
/// and return its packed `(ptr<<32)|len`. Used by the early-out paths
/// that never reach the protect / classify steps.
#[cfg(mruby_linked)]
fn write_error_response(class: &str, message: impl Into<String>, backtrace: Vec<String>) -> u64 {
    let bytes = encode_error_bytes(class, &message.into(), backtrace);
    write_yield_buffer(&bytes)
}

/// Allocate a `len`-byte buffer via `__kobako_alloc` inside the active
/// wasm instance, copy `bytes` into it, and return the packed
/// `(ptr<<32)|len` u64 the host reads.
#[cfg(mruby_linked)]
fn write_yield_buffer(bytes: &[u8]) -> u64 {
    let len_u32 = match u32::try_from(bytes.len()) {
        Ok(n) => n,
        Err(_) => return 0,
    };
    let ptr = kobako_core::abi::alloc(len_u32);
    if ptr == 0 || len_u32 == 0 {
        return 0;
    }
    // SAFETY: `__kobako_alloc` returned a `len_u32`-byte buffer in the
    // current Instance's linear memory; copying `bytes.len()` bytes
    // into it is in-bounds.
    unsafe {
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr as *mut u8, bytes.len());
    }
    pack_ptr_len(ptr, len_u32)
}
