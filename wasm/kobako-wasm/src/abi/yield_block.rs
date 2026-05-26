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
//! 2. Resolve the active `mrb_state` via the per-invocation `MRB`
//!    slot and read the topmost block off `BLOCK_STACK`
//!    ({docs/behavior.md B-23 / B-28}[link:../../../../docs/behavior.md]).
//! 3. Convert codec args → `mrb_value` args via the standard runtime
//!    converter, then invoke `mrb_yield_argv` inside
//!    `mrb_protect_error` so any guest-side raise (or `break` /
//!    Proc-`return` RBreak) lands as `Err(exc)` instead of
//!    long-jumping past the Rust frame
//!    ({docs/behavior.md E-21}[link:../../../../docs/behavior.md]).
//! 4. Encode the outcome as a `YieldResponse`:
//!     * normal return → `tag 0x01` ok carrying the value through the
//!       standard codec
//!     * raised exception (including RBreak in S5b — the
//!       `ci_break_index` discrimination that splits real `break`
//!       from a Proc-`return` lands in S6a) → `tag 0x04` error with
//!       `{class, message, backtrace}`
//! 5. Allocate the response buffer via `__kobako_alloc`, copy the
//!    bytes in, return the packed `(ptr<<32)|len`.

#[cfg(target_arch = "wasm32")]
use super::pack_u64;

/// Reactor entry — see module docs. Signature pinned by
/// docs/wire-codec.md § ABI Signatures (5 guest exports).
#[no_mangle]
pub extern "C" fn __kobako_yield_to_block(req_ptr: i32, req_len: i32) -> u64 {
    #[cfg(target_arch = "wasm32")]
    {
        yield_to_block_body(req_ptr, req_len)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        // Host stub — see `__kobako_run` for the shape rationale.
        let _ = req_ptr;
        let _ = req_len;
        0
    }
}

#[cfg(target_arch = "wasm32")]
fn yield_to_block_body(req_ptr: i32, req_len: i32) -> u64 {
    use super::block_stack::BLOCK_STACK;
    use super::mrb_slot::MRB;
    use crate::kobako::Kobako;
    use crate::mruby::sys;

    // Step 1: decode positional args off the request buffer.
    let args_codec = match decode_yield_args(req_ptr, req_len) {
        Ok(items) => items,
        Err(msg) => return write_error_response("Kobako::Transport::Error", msg, Vec::new()),
    };

    // Step 2: resolve the active VM + Kobako runtime + bound block.
    let Some(mrb) = MRB.as_ref() else {
        return write_error_response(
            "RuntimeError",
            "yield_to_block invoked without an active Sandbox invocation",
            Vec::new(),
        );
    };
    // SAFETY: MRB is `Some` only after `Kobako::install` ran for the
    // current invocation; `resolve_raw`'s precondition is satisfied.
    let kobako = unsafe { Kobako::resolve_raw(mrb.as_ptr()) };
    let Some(block) = BLOCK_STACK.last() else {
        return write_error_response(
            "LocalJumpError",
            "yield_to_block invoked without a block on the stack",
            Vec::new(),
        );
    };

    // Step 3: convert codec args → mrb_value args.
    let mrb_args: Vec<sys::mrb_value> = args_codec
        .into_iter()
        .map(|v| kobako.to_mrb_value(v).as_raw())
        .collect();

    // Step 4: protected yield. `mrb_yield_argv` raises via `MRB_THROW`
    // for break / return / raise; `mrb.protect` installs a local
    // `c_jmp` that catches it and surfaces the exception value as
    // `Err`. Snapshot the current callinfo index *before* the
    // protected call so step 5's classification can place any RBreak
    // destination relative to this yielder's frame.
    let mrb_ptr = mrb.as_ptr();
    let block_raw = block.as_raw();
    let argc = mrb_args.len() as i32;
    let argv_ptr = mrb_args.as_ptr();
    // SAFETY: `mrb_ptr` is live by the outer `&Mrb` borrow; the shim
    // reads only the public `mrb_context.ci` / `cibase` fields.
    let enter_idx = unsafe { sys::mrb_current_ci_index_func(mrb_ptr) };
    let result = mrb.protect(|_inner| {
        // SAFETY: `mrb_ptr` is live by the outer `&Mrb` borrow;
        // `block_raw` was pushed onto BLOCK_STACK by the still-active
        // bridge frame, which roots it via mruby's call frame argv;
        // `argv_ptr` / `argc` point into the outer `mrb_args` Vec
        // which outlives this closure.
        let raw = unsafe { sys::mrb_yield_argv(mrb_ptr, block_raw, argc, argv_ptr) };
        sys::Value::from_raw(raw)
    });

    // Step 5: encode the outcome. Extract any exception fields
    // immediately on the Err path before any other mruby allocation
    // could sweep the exception object out of the GC arena. RBreak
    // outcomes split on `ci_break_index` vs `enter_idx` per B-25 / E-21.
    let bytes = match result {
        Ok(value) => encode_ok_response(&kobako, value),
        Err(exc) => classify_protected_error(&kobako, mrb, exc, enter_idx),
    };
    write_yield_buffer(&bytes)
}

/// Classify the value `mrb_protect_error` surfaced on its `Err` path
/// into a YieldResponse. mruby's vm.c already raises
/// `E_LOCALJUMP_ERROR` directly for the orphan-block / orphan-Proc
/// shapes (vm.c:2756 / 2776), so any RBreak we see here is either a
/// real `break` from a non-lambda block or a non-orphan Proc `return`
/// — discriminate them by comparing `RBreak.ci_break_index` against
/// the `enter_idx` snapshot taken immediately before the protected
/// yield.
#[cfg(target_arch = "wasm32")]
fn classify_protected_error(
    kobako: &crate::kobako::Kobako,
    mrb: &crate::mruby::Mrb,
    exc: crate::mruby::sys::Value,
    enter_idx: usize,
) -> Vec<u8> {
    use crate::mruby::sys;
    // SAFETY: `mrb_break_p_func` only reads the value's type tag,
    // safe on any mrb_value.
    if !unsafe { sys::mrb_break_p_func(exc.as_raw()) } {
        return encode_error_response_from_exception(kobako, mrb, exc);
    }
    // SAFETY: `mrb_break_p_func` returned non-zero, so `exc` is a
    // valid RBreak-tagged value and the cast inside the shim is sound.
    let brk_idx = unsafe { sys::mrb_break_ci_index_func(exc.as_raw()) };
    if brk_idx >= enter_idx {
        // SAFETY: same gate as `mrb_break_ci_index_func` above.
        let brk_val_raw = unsafe { sys::mrb_break_value_func(exc.as_raw()) };
        encode_break_response(kobako, sys::Value::from_raw(brk_val_raw))
    } else {
        // RBreak whose destination is deeper than the yielder's frame
        // is a non-orphan Proc `return` aimed at an outer guest method
        // — unrepresentable across the host yield boundary (E-21).
        encode_error_bytes(
            "LocalJumpError",
            "unexpected return across a Kobako yield boundary",
            Vec::new(),
        )
    }
}

#[cfg(target_arch = "wasm32")]
fn encode_break_response(
    kobako: &crate::kobako::Kobako,
    value: crate::mruby::sys::Value,
) -> Vec<u8> {
    use crate::yield_response::{encode_response, Response, TAG_BREAK};
    let codec_value = kobako.to_codec_value(value);
    let resp = Response {
        tag: TAG_BREAK,
        value: codec_value,
    };
    match encode_response(&resp) {
        Ok(bytes) => bytes,
        Err(_) => encode_error_bytes(
            "Kobako::Transport::Error",
            "failed to encode break value",
            Vec::new(),
        ),
    }
}

#[cfg(target_arch = "wasm32")]
fn decode_yield_args(req_ptr: i32, req_len: i32) -> Result<Vec<crate::codec::Value>, String> {
    use crate::codec::{Decoder, Value};
    // SAFETY: `req_ptr` / `req_len` were produced by the host's
    // `Instance#yield_to_block`, which allocates the buffer via
    // `__kobako_alloc` inside this same wasm instance and writes the
    // encoded args bytes in. Reading `req_len` bytes from `req_ptr`
    // is in-bounds for the current Instance's linear memory.
    let bytes: &[u8] = if req_len == 0 {
        &[]
    } else {
        unsafe { core::slice::from_raw_parts(req_ptr as usize as *const u8, req_len as usize) }
    };
    let mut dec = Decoder::new(bytes);
    let frame = dec
        .read_value()
        .map_err(|e| format!("failed to decode yield args: {e}"))?;
    match frame {
        Value::Array(items) => Ok(items),
        _ => Err("yield args must be a msgpack array".to_string()),
    }
}

#[cfg(target_arch = "wasm32")]
fn encode_ok_response(kobako: &crate::kobako::Kobako, value: crate::mruby::sys::Value) -> Vec<u8> {
    use crate::yield_response::{encode_response, Response, TAG_OK};
    let codec_value = kobako.to_codec_value(value);
    let resp = Response {
        tag: TAG_OK,
        value: codec_value,
    };
    match encode_response(&resp) {
        Ok(bytes) => bytes,
        Err(_) => encode_error_bytes(
            "Kobako::Transport::Error",
            "failed to encode yield ok value",
            Vec::new(),
        ),
    }
}

#[cfg(target_arch = "wasm32")]
fn encode_error_response_from_exception(
    kobako: &crate::kobako::Kobako,
    mrb: &crate::mruby::Mrb,
    exc: crate::mruby::sys::Value,
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

#[cfg(target_arch = "wasm32")]
fn encode_error_bytes(class: &str, message: &str, backtrace: Vec<String>) -> Vec<u8> {
    use crate::codec::Value;
    use crate::yield_response::{encode_response, Response, TAG_ERROR};
    let payload = Value::Map(vec![
        (Value::Str("class".into()), Value::Str(class.into())),
        (Value::Str("message".into()), Value::Str(message.into())),
        (
            Value::Str("backtrace".into()),
            Value::Array(backtrace.into_iter().map(Value::Str).collect()),
        ),
    ]);
    let resp = Response {
        tag: TAG_ERROR,
        value: payload,
    };
    encode_response(&resp).unwrap_or_default()
}

/// Write an error YieldResponse directly into a fresh guest buffer
/// and return its packed `(ptr<<32)|len`. Used by the early-out paths
/// that never reach the protect / classify steps.
#[cfg(target_arch = "wasm32")]
fn write_error_response(class: &str, message: impl Into<String>, backtrace: Vec<String>) -> u64 {
    let bytes = encode_error_bytes(class, &message.into(), backtrace);
    write_yield_buffer(&bytes)
}

/// Allocate a `len`-byte buffer via `__kobako_alloc` inside the active
/// wasm instance, copy `bytes` into it, and return the packed
/// `(ptr<<32)|len` u64 the host reads.
#[cfg(target_arch = "wasm32")]
fn write_yield_buffer(bytes: &[u8]) -> u64 {
    let len_u32 = match u32::try_from(bytes.len()) {
        Ok(n) => n,
        Err(_) => return 0,
    };
    let ptr = super::outcome_buffer::__kobako_alloc(len_u32);
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
