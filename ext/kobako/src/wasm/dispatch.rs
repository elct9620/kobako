//! Host-side dispatch for the `__kobako_rpc_call` import.
//!
//! When the guest invokes the wasm import declared in
//! `wasm/kobako-wasm/src/abi.rs`, wasmtime calls back into the host
//! through the closure built in [`super::instance::build_instance`].
//! That closure delegates here. The dispatcher (SPEC.md B-12 / B-13):
//!
//!   1. Reads the Request bytes from guest linear memory.
//!   2. Hands them to the Ruby-side `Kobako::Registry` and recovers
//!      Response bytes.
//!   3. Allocates a guest buffer via `__kobako_alloc(len)` invoked
//!      through `Caller::get_export`.
//!   4. Writes the Response bytes into the guest buffer.
//!   5. Returns packed `(ptr<<32)|len` for the guest to decode.
//!
//! Returns 0 on any step failure. `Kobako::Sandbox` always installs a
//! Registry before invoking the guest, so reaching the dispatcher with
//! no Registry bound is itself a wire-layer fault; the guest maps a 0
//! return to a trap. Failures during normal dispatch surface as
//! Response.err envelopes from the Registry itself — they never reach
//! this 0-return path.

use magnus::value::{Opaque, ReprValue};
use magnus::{Error as MagnusError, RString, Ruby, Value};
use wasmtime::{Caller, Extern};

use super::host_state::HostState;

/// Drive a single `__kobako_rpc_call` invocation end-to-end. Entry point
/// from the wasmtime closure built in [`super::instance::build_instance`].
pub(crate) fn dispatch_rpc(caller: &mut Caller<'_, HostState>, req_ptr: i32, req_len: i32) -> i64 {
    let req_bytes = match read_caller_memory(caller, req_ptr, req_len) {
        Some(b) => b,
        None => return 0,
    };

    // No Registry bound — return 0 to signal a wire-layer fault; the guest
    // maps a 0 return to a trap. `Kobako::Sandbox` always installs a
    // Registry before invoking the guest, so reaching this branch indicates
    // a misuse rather than a normal control path.
    let registry = match caller.data().registry {
        Some(d) => d,
        None => return 0,
    };

    let resp_bytes = match invoke_registry(registry, &req_bytes) {
        Ok(b) => b,
        Err(_) => return 0,
    };

    write_response(caller, &resp_bytes).unwrap_or(0)
}

/// Call the Ruby Registry's `#dispatch(request_bytes)` method and return
/// the encoded Response bytes. Errors here mean the Registry itself
/// failed (it is contracted never to raise — see
/// `Kobako::Registry#dispatch`), which we treat as a wire-layer fault.
fn invoke_registry(registry: Opaque<Value>, req_bytes: &[u8]) -> Result<Vec<u8>, MagnusError> {
    // The wasmtime callback runs on the same Ruby thread that called
    // Sandbox#run — the invariant SPEC Implementation Standards
    // Architecture pins for the host gem — so `Ruby::get()` is always
    // available here. Panicking with `expect` localises the violation
    // rather than letting a nonsense error propagate.
    let ruby = Ruby::get().expect("Ruby handle unavailable in __kobako_rpc_call");
    let registry_value: Value = ruby.get_inner(registry);
    let req_str = ruby.str_from_slice(req_bytes);
    let resp: RString = registry_value.funcall("dispatch", (req_str,))?;
    // SAFETY: the returned RString is held by the Ruby VM for the duration of
    // this scope; copying its bytes into a Vec is a defensive standard pattern.
    let bytes = unsafe { resp.as_slice() }.to_vec();
    Ok(bytes)
}

/// Allocate a guest-side buffer through `__kobako_alloc` and copy the
/// response bytes into it. Returns the packed `(ptr<<32)|len` u64.
fn write_response(caller: &mut Caller<'_, HostState>, bytes: &[u8]) -> Option<i64> {
    let alloc = match caller.get_export("__kobako_alloc") {
        Some(Extern::Func(f)) => f.typed::<i32, i32>(&*caller).ok()?,
        _ => return None,
    };
    let len_i32 = i32::try_from(bytes.len()).ok()?;
    let ptr = alloc.call(&mut *caller, len_i32).ok()?;
    if ptr == 0 {
        return None;
    }

    let mem = match caller.get_export("memory") {
        Some(Extern::Memory(m)) => m,
        _ => return None,
    };
    mem.write(&mut *caller, ptr as usize, bytes).ok()?;

    let ptr_u32 = ptr as u32;
    let len_u32 = bytes.len() as u32;
    Some(((ptr_u32 as i64) << 32) | (len_u32 as i64))
}

/// Copy `[ptr, ptr+len)` out of the guest's linear memory as seen from
/// `caller`. Returns `None` when `memory` is not exported or the slice
/// falls outside the live memory range.
fn read_caller_memory(caller: &mut Caller<'_, HostState>, ptr: i32, len: i32) -> Option<Vec<u8>> {
    let mem = match caller.get_export("memory") {
        Some(Extern::Memory(m)) => m,
        _ => return None,
    };
    let data = mem.data(&caller);
    let start = ptr as usize;
    let end = start.checked_add(len as usize)?;
    data.get(start..end).map(|s| s.to_vec())
}
