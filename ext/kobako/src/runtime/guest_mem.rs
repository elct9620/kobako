//! Caller-based guest linear-memory I/O shared by the host-import paths.
//!
//! Both directions of a host↔guest buffer handoff that run *inside* a wasm
//! callback frame go through here: writing the transport Response back
//! (`super::dispatch`) and shipping block-yield args into the guest
//! ([`drive_yield`], below) performed the same `__kobako_alloc` +
//! bounds-check + `memory.write` dance with only the diagnostic strings
//! differing. The Store-based write path (`Runtime::write_envelope`) is a
//! separate beast — it holds the cached `Store`, not a `Caller` — and stays
//! in `runtime.rs`.

use wasmtime::{Caller, Extern, Memory};

use super::invocation::Invocation;

/// Resolve the guest's exported linear `memory`. The lookup shape (and its
/// diagnostic) is shared by every Caller-based path here — the write side
/// ([`alloc_and_write`]), the read side ([`read`]), and the yield round-trip
/// ([`drive_yield`]) — so the "no linear memory" reason lives in one place.
/// `read` maps the `Err` to its own `None` outcome via `.ok()`.
fn memory_export(caller: &mut Caller<'_, Invocation>) -> Result<Memory, &'static str> {
    match caller.get_export("memory") {
        Some(Extern::Memory(m)) => Ok(m),
        _ => Err("Sandbox runtime does not export linear memory"),
    }
}

/// Allocate `bytes.len()` bytes in guest memory via `__kobako_alloc` and
/// copy `bytes` in. Returns the guest pointer. Every failure path carries a
/// `&'static str` reason so the caller can surface a diagnostic rather than
/// a silent fault.
pub(super) fn alloc_and_write(
    caller: &mut Caller<'_, Invocation>,
    bytes: &[u8],
) -> Result<u32, &'static str> {
    let alloc = match caller.get_export("__kobako_alloc") {
        Some(Extern::Func(f)) => f
            .typed::<i32, i32>(&*caller)
            .map_err(|_| "Sandbox runtime's allocation hook has the wrong signature")?,
        _ => return Err("Sandbox runtime is missing the allocation hook"),
    };
    let len = i32::try_from(bytes.len()).map_err(|_| "guest buffer exceeds 2 GiB")?;
    let ptr = alloc
        .call(&mut *caller, len)
        .map_err(|_| "Sandbox allocation trapped while preparing a guest buffer")?;
    if ptr == 0 {
        return Err("Sandbox is out of memory while preparing a guest buffer");
    }

    let mem = memory_export(caller)?;
    mem.write(&mut *caller, ptr as usize, bytes)
        .map_err(|_| "could not write into Sandbox memory (range invalid)")?;
    Ok(ptr as u32)
}

/// Copy `[ptr, ptr + len)` out of the guest's linear memory as seen from
/// `caller`. Returns `None` when `memory` is not exported or the slice
/// falls outside the live memory range.
pub(super) fn read(caller: &mut Caller<'_, Invocation>, ptr: i32, len: i32) -> Option<Vec<u8>> {
    let mem = memory_export(caller).ok()?;
    let data = mem.data(&caller);
    let start = ptr as usize;
    let end = start.checked_add(len as usize)?;
    data.get(start..end).map(|s| s.to_vec())
}

/// Validate the invocation envelope length and return it as `i32` — the
/// signed wasm ABI parameter type for the guest-run entrypoint. Rejects
/// sizes above `i32::MAX` (2 GiB) so the downstream cast cannot silently
/// wrap.
pub(super) fn envelope_len_to_i32(len: usize) -> Result<i32, &'static str> {
    i32::try_from(len).map_err(|_| "invocation payload exceeds 2 GiB")
}

/// Compute the half-open range `[ptr, ptr + len)` for a guest linear-memory
/// copy, validating that the arithmetic does not overflow and the range
/// fits inside `mem_size`. Shared by `Runtime::write_envelope` (write side)
/// and `Runtime::fetch_outcome_bytes` (read side).
pub(super) fn guest_buffer_range(
    ptr: usize,
    len: usize,
    mem_size: usize,
) -> Result<core::ops::Range<usize>, &'static str> {
    let end = ptr.checked_add(len).ok_or("ptr + len overflow")?;
    if end > mem_size {
        return Err("range exceeds Sandbox memory size");
    }
    Ok(ptr..end)
}

/// Unpack the `(ptr, len)` u64 returned by `__kobako_take_outcome`:
/// high 32 bits = ptr, low 32 bits = len. Mirrors the guest-side
/// `crate::abi::unpack_u64` in `wasm/kobako-wasm/src/abi.rs`.
pub(super) fn unpack_outcome_packed(packed: u64) -> (usize, usize) {
    let ptr = (packed >> 32) as u32 as usize;
    let len = packed as u32 as usize;
    (ptr, len)
}

/// Allocate `args.len()` bytes in guest memory, copy the args payload in,
/// call `__kobako_yield_to_block(ptr, len)`, then read the response slice
/// the guest produced and return it. Mirrors `dispatch::write_response`'s
/// allocator dance but in the opposite direction — the host is the
/// *initiator* of this round-trip, not the responder.
pub(super) fn drive_yield(
    caller: &mut Caller<'_, Invocation>,
    args: &[u8],
) -> Result<Vec<u8>, &'static str> {
    let len_i32 = i32::try_from(args.len()).map_err(|_| "yield args exceed 2 GiB")?;
    let req_ptr = alloc_and_write(caller, args)? as i32;

    let yield_fn = match caller.get_export("__kobako_yield_to_block") {
        Some(Extern::Func(f)) => f
            .typed::<(i32, i32), u64>(&*caller)
            .map_err(|_| "Sandbox runtime's yield hook has the wrong signature")?,
        _ => return Err("Sandbox runtime is missing the yield hook"),
    };
    let packed = yield_fn
        .call(&mut *caller, (req_ptr, len_i32))
        .map_err(|_| "Sandbox trapped during yield_to_block")?;
    let (resp_ptr, resp_len) = unpack_outcome_packed(packed);
    if resp_len == 0 {
        return Err("Sandbox returned an empty YieldResponse (wire violation)");
    }

    let mem = memory_export(caller)?;
    let data = mem.data(&caller);
    let range = guest_buffer_range(resp_ptr, resp_len, data.len())
        .map_err(|_| "YieldResponse buffer is out of bounds")?;
    Ok(data[range].to_vec())
}

#[cfg(test)]
mod tests {
    use super::{envelope_len_to_i32, guest_buffer_range, unpack_outcome_packed};

    #[test]
    fn envelope_len_to_i32_accepts_zero_and_max() {
        assert_eq!(envelope_len_to_i32(0), Ok(0));
        assert_eq!(envelope_len_to_i32(i32::MAX as usize), Ok(i32::MAX));
    }

    #[test]
    fn envelope_len_to_i32_rejects_past_i32_max() {
        assert!(envelope_len_to_i32(i32::MAX as usize + 1).is_err());
        assert!(envelope_len_to_i32(usize::MAX).is_err());
    }

    #[test]
    fn guest_buffer_range_returns_half_open_range() {
        assert_eq!(guest_buffer_range(10, 5, 100), Ok(10..15));
    }

    #[test]
    fn guest_buffer_range_accepts_zero_length_at_any_in_bounds_ptr() {
        assert_eq!(guest_buffer_range(0, 0, 0), Ok(0..0));
        assert_eq!(guest_buffer_range(42, 0, 100), Ok(42..42));
    }

    #[test]
    fn guest_buffer_range_rejects_ptr_plus_len_overflow() {
        assert!(guest_buffer_range(usize::MAX, 1, usize::MAX).is_err());
    }

    #[test]
    fn guest_buffer_range_rejects_end_past_memory() {
        assert!(guest_buffer_range(10, 100, 50).is_err());
        assert_eq!(guest_buffer_range(0, 50, 50), Ok(0..50));
    }

    #[test]
    fn unpack_outcome_packed_extracts_high_ptr_low_len() {
        assert_eq!(
            unpack_outcome_packed(0xAABB_CCDD_1122_3344),
            (0xAABB_CCDD, 0x1122_3344)
        );
    }

    #[test]
    fn unpack_outcome_packed_zero_decodes_to_zero_pair() {
        assert_eq!(unpack_outcome_packed(0), (0, 0));
    }
}
