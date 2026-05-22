//! `__kobako_yield_to_block` — host-initiated re-entry into a guest
//! block (docs/wire-codec.md § ABI Signatures, docs/behavior.md B-24).
//!
//! The host calls this from inside a `__kobako_dispatch` callback when
//! a Service method invokes its yield proxy. The signature mirrors
//! `__kobako_dispatch`'s — `(req_ptr, req_len) -> i64` with the same
//! packed-u64 `(ptr<<32)|len` return — so the same alloc / write /
//! read shape applies in the symmetric direction.
//!
//! ## S4 stub
//!
//! The body in this stage emits a fixed `YieldResponse` `tag 0x04`
//! `error("not implemented")` envelope and returns it through the
//! standard `__kobako_alloc` + copy + packed-u64 path. The point is to
//! exercise the new ABI export against the host plumbing (thread-local
//! `Caller` + magnus `Instance#yield_to_block`) before the real yield
//! body (S5b) lands. The host magnus method observes the stub error
//! and surfaces it as a controlled Ruby exception until the real
//! mruby `mrb_yield_argv` path replaces this body.

#[cfg(target_arch = "wasm32")]
use super::pack_u64;

/// Reactor entry — see module docs. Signature pinned by
/// docs/wire-codec.md § ABI Signatures (5 guest exports).
#[no_mangle]
pub extern "C" fn __kobako_yield_to_block(req_ptr: i32, req_len: i32) -> u64 {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = req_ptr;
        let _ = req_len;
        yield_to_block_body()
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
fn yield_to_block_body() -> u64 {
    use crate::codec::Value;
    use crate::yield_response::{encode_response, Response, TAG_ERROR};

    // Stub payload — the eventual S5b body classifies the real
    // `mrb_yield_argv` outcome into tag 0x01 / 0x02 / 0x04. Tag 0x04
    // with a {class, message, backtrace} map is the shape both sides
    // already agreed on at S2b.
    let payload = Value::Map(vec![
        (
            Value::Str("class".into()),
            Value::Str("NotImplementedError".into()),
        ),
        (
            Value::Str("message".into()),
            Value::Str("__kobako_yield_to_block is not yet implemented".into()),
        ),
        (Value::Str("backtrace".into()), Value::Array(Vec::new())),
    ]);
    let resp = Response {
        tag: TAG_ERROR,
        value: payload,
    };
    let bytes = match encode_response(&resp) {
        Ok(b) => b,
        // Wire violation — host walks trap path on len == 0.
        Err(_) => return 0,
    };

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
    // SAFETY: `__kobako_alloc` is a guest export defined in
    // `super::outcome_buffer`; calling it from within the same guest
    // module is a direct Rust call — no FFI boundary, no UB risk.
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
