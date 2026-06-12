//! `__kobako_yield_to_block` — host-initiated re-entry into a guest
//! block (docs/wire-codec.md § ABI Signatures, docs/behavior.md B-24).
//!
//! The host calls this from inside a `__kobako_dispatch` callback when
//! a Service method invokes its Yielder. The signature mirrors
//! `__kobako_dispatch`'s — `(req_ptr, req_len) -> i64` with the same
//! packed-u64 `(ptr<<32)|len` return — so the same alloc / write /
//! read shape applies in the symmetric direction.
//!
//! ## Body
//!
//! 1. Decode the yield arguments (msgpack array of positional args)
//!    out of the request buffer.
//! 2. Resolve the active `mrb_state` via the module-level `MRB`
//!    slot and read the topmost block off `BLOCK_STACK`
//!    (docs/behavior.md B-23 / B-28).
//! 3. Convert codec args → `Value` args via the standard runtime
//!    converter, then yield to the block through beni's protected
//!    `Proc::call` so any guest-side raise (or `break` / Proc-`return`
//!    RBreak) lands as `Err` instead of long-jumping past the Rust
//!    frame (docs/behavior.md E-21).
//! 4. Encode the outcome as a `YieldResponse`:
//!     * normal return of a wire-representable value → `tag 0x01` ok
//!       carrying the value through the standard codec
//!     * a real `break` from a non-lambda block → `tag 0x02` break
//!       (docs/behavior.md B-25)
//!     * a raised exception, a return value with no wire representation
//!       (docs/behavior.md E-22),
//!       or an RBreak aimed past the yielder's frame (a non-orphan Proc
//!       `return`, docs/behavior.md E-21)
//!       → `tag 0x04` error with `{class, message, backtrace}`
//! 5. Allocate the response buffer via `__kobako_alloc`, copy the
//!    bytes in, return the packed `(ptr<<32)|len`.

#[cfg(mruby_linked)]
use kobako_core::abi::pack_u64;

/// Invocation entry behind the `__kobako_yield_to_block` export —
/// see module docs. Signature pinned by docs/wire-codec.md § ABI
/// Signatures (5 guest exports).
pub(crate) fn yield_to_block(req: &[u8]) -> u64 {
    #[cfg(mruby_linked)]
    {
        yield_to_block_body(req)
    }
    #[cfg(not(mruby_linked))]
    {
        let _ = req;
        crate::not_linked()
    }
}

#[cfg(mruby_linked)]
fn yield_to_block_body(req: &[u8]) -> u64 {
    use super::mrb_slot::MRB;
    use crate::runtime::block_stack::BLOCK_STACK;
    use crate::runtime::Kobako;
    use beni::{sys, FromValue, Proc};

    // Step 1: decode positional args off the request buffer.
    let args_codec = match decode_yield_args(req) {
        Ok(items) => items,
        Err(msg) => return write_error_response("Kobako::Transport::Error", msg, Vec::new()),
    };

    // Step 2: resolve the active VM + Kobako runtime + bound block.
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

    // Step 3: convert codec args → Value args.
    let args: Vec<beni::Value> = args_codec
        .into_iter()
        .map(|v| kobako.to_mrb_value(v))
        .collect();

    // Step 4: protected yield via beni's `Proc::call`, which folds the
    // `mrb_yield_argv` + protect machinery — a guest-side raise / break /
    // Proc-`return` surfaces as `Err` instead of long-jumping past the
    // Rust frame. Snapshot the current callinfo index *before* the call
    // so step 5's classification can place any RBreak destination
    // relative to this yielder's frame.
    // SAFETY: `mrb` is live by the outer `&Mrb` borrow; the shim reads
    // the VM-internal `mrb_context.ci` / `cibase` frame indices, which
    // carry no MRB_API accessor and so stay on the unsafe `sys` seam.
    let enter_idx = unsafe { sys::mrb_current_ci_index_func(mrb.as_ptr()) };
    let result = block.call(mrb, &args);

    // Step 5: encode the outcome. Extract any exception fields
    // immediately on the Err path before any other mruby allocation
    // could sweep the exception object out of the GC arena. RBreak
    // outcomes split on `ci_break_index` vs `enter_idx` per B-25 / E-21.
    let bytes = match result {
        Ok(value) => encode_ok_response(&kobako, value),
        Err(beni::Error::Exception(exc)) => classify_protected_error(&kobako, mrb, exc, enter_idx),
        // A Rust panic inside the protected yield can only surface
        // here under unwinding panics; the guest builds with
        // `panic = "abort"`, so this arm is unreachable in production.
        Err(beni::Error::Panic(_)) => std::process::abort(),
    };
    write_yield_buffer(&bytes)
}

/// Classify the value the protected `Proc::call` surfaced on its `Err`
/// path into a YieldResponse. mruby's vm.c already raises
/// `E_LOCALJUMP_ERROR` directly for the orphan-block / orphan-Proc
/// shapes (vm.c:2756 / 2776), so any RBreak we see here is either a
/// real `break` from a non-lambda block or a non-orphan Proc `return`
/// — discriminate them by comparing `RBreak.ci_break_index` against
/// the `enter_idx` snapshot taken immediately before the protected
/// yield.
#[cfg(mruby_linked)]
fn classify_protected_error(
    kobako: &crate::runtime::Kobako,
    mrb: &beni::Mrb,
    exc: beni::Value,
    enter_idx: usize,
) -> Vec<u8> {
    use beni::sys;
    // A non-break exception is a plain raise — tag 0x04.
    let Some(brk) = exc.as_break() else {
        return encode_error_response_from_exception(kobako, mrb, exc);
    };
    // SAFETY: `exc` is RBreak-tagged (`as_break` returned `Some`); the
    // shim reads `RBreak.ci_break_index`, a VM-internal field with no
    // MRB_API accessor, so it stays on the unsafe `sys` seam.
    let brk_idx = unsafe { sys::mrb_break_ci_index_func(exc.as_raw()) };
    if brk_idx >= enter_idx {
        encode_break_response(kobako, brk.value())
    } else {
        // RBreak whose destination is deeper than the yielder's frame
        // is a non-orphan Proc `return` aimed at an outer guest method
        // — unrepresentable across the host yield boundary (E-21).
        encode_error_bytes(
            "LocalJumpError",
            "cannot return from a block passed into the Sandbox",
            Vec::new(),
        )
    }
}

#[cfg(mruby_linked)]
fn encode_break_response(kobako: &crate::runtime::Kobako, value: beni::Value) -> Vec<u8> {
    use kobako_core::codec::Encode;
    use kobako_core::transport::{Yield, TAG_BREAK};
    let Some(codec_value) = kobako.try_codec_value(value) else {
        // `break val` whose value has no wire representation is the
        // E-22 shape on the break path — surface it as a 0x04 error
        // rather than coerce the value to a String.
        return encode_error_bytes(
            "TypeError",
            &format!(
                "break value of type {} is not a supported sandbox value type",
                value.classname(kobako.mrb())
            ),
            Vec::new(),
        );
    };
    let resp = Yield {
        tag: TAG_BREAK,
        value: codec_value,
    };
    match resp.encode() {
        Ok(bytes) => bytes,
        Err(_) => encode_error_bytes(
            "Kobako::Transport::Error",
            "failed to encode break value",
            Vec::new(),
        ),
    }
}

#[cfg(mruby_linked)]
fn decode_yield_args(req: &[u8]) -> Result<Vec<kobako_core::codec::Value>, String> {
    use kobako_core::codec::{Decoder, Value};
    let mut dec = Decoder::new(req);
    let frame = dec
        .read_value()
        .map_err(|e| format!("failed to decode the block arguments: {e}"))?;
    match frame {
        Value::Array(items) => Ok(items),
        _ => Err("block arguments must be an array".to_string()),
    }
}

#[cfg(mruby_linked)]
fn encode_ok_response(kobako: &crate::runtime::Kobako, value: beni::Value) -> Vec<u8> {
    use kobako_core::codec::Encode;
    use kobako_core::transport::{Yield, TAG_OK};
    let Some(codec_value) = kobako.try_codec_value(value) else {
        // A block returning a value with no wire representation is E-22.
        // The host Yielder reifies this 0x04 error as an exception at the
        // Service's yield site instead of receiving a misleading String.
        return encode_error_bytes(
            "TypeError",
            &format!(
                "block return value of type {} is not a supported sandbox value type",
                value.classname(kobako.mrb())
            ),
            Vec::new(),
        );
    };
    let resp = Yield {
        tag: TAG_OK,
        value: codec_value,
    };
    match resp.encode() {
        Ok(bytes) => bytes,
        Err(_) => encode_error_bytes(
            "Kobako::Transport::Error",
            "failed to encode yield ok value",
            Vec::new(),
        ),
    }
}

#[cfg(mruby_linked)]
fn encode_error_response_from_exception(
    kobako: &crate::runtime::Kobako,
    mrb: &beni::Mrb,
    exc: beni::Value,
) -> Vec<u8> {
    // Mirror `boot::take_pending_panic` field order: classname →
    // message → backtrace. Each step uses `exc` while it is still
    // GC-reachable in mruby's arena.
    let class_name = {
        let cn = exc.classname(mrb);
        if cn.is_empty() {
            "RuntimeError".to_string()
        } else {
            cn.to_string()
        }
    };
    let message = {
        let msg_val = exc.call(mrb, c"message", &[]);
        let m = msg_val.to_string(mrb);
        if m.is_empty() {
            class_name.clone()
        } else {
            m
        }
    };
    let backtrace = kobako.extract_backtrace(exc);
    encode_error_bytes(&class_name, &message, backtrace)
}

#[cfg(mruby_linked)]
fn encode_error_bytes(class: &str, message: &str, backtrace: Vec<String>) -> Vec<u8> {
    use kobako_core::codec::Encode;
    use kobako_core::codec::Value;
    use kobako_core::transport::{Yield, TAG_ERROR};
    let payload = Value::Map(vec![
        (Value::Str("class".into()), Value::Str(class.into())),
        (Value::Str("message".into()), Value::Str(message.into())),
        (
            Value::Str("backtrace".into()),
            Value::Array(backtrace.into_iter().map(Value::Str).collect()),
        ),
    ]);
    let resp = Yield {
        tag: TAG_ERROR,
        value: payload,
    };
    resp.encode().unwrap_or_default()
}

/// Write an error YieldResponse directly into a fresh guest buffer
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
    pack_u64(ptr, len_u32)
}
