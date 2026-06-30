//! Caller-based guest linear-memory I/O shared by the host-import paths.
//!
//! Both directions of a host↔guest buffer handoff that run *inside* a wasm
//! callback frame go through here: writing the transport Response back
//! (`super::dispatch`) and shipping block-yield args into the guest
//! (`drive_yield`, below) performed the same `__kobako_alloc` +
//! bounds-check + `memory.write` dance with only the diagnostic strings
//! differing. The Store-based write path (`Runtime::write_envelope`) is a
//! separate beast — it holds the cached `Store`, not a `Caller` — and stays
//! in `runtime.rs`.

use wasmtime::{Caller, Extern, Memory};

use super::invocation::Invocation;
use crate::contract::error::Trap;
use crate::contract::yielder::Yielder;

/// The wasmtime-backed `Yielder` (`crate::contract::yielder`): a
/// frame-scoped wrapper over the dispatch `Caller` that drives a block-yield
/// round-trip through `drive_yield`. Built per `__kobako_dispatch` frame and
/// handed to the dispatch handler, so nested dispatch frames (B-28) each
/// carry their own and stack on the Rust call stack with no shared slot.
pub(super) struct CallerYielder<'a, 'c> {
    caller: &'a mut Caller<'c, Invocation>,
}

impl<'a, 'c> CallerYielder<'a, 'c> {
    pub(super) fn new(caller: &'a mut Caller<'c, Invocation>) -> Self {
        Self { caller }
    }
}

impl Yielder for CallerYielder<'_, '_> {
    fn yield_block(&mut self, args: &[u8]) -> Result<Vec<u8>, Trap> {
        drive_yield(self.caller, args).map_err(|msg| Trap::Other(msg.to_string()))
    }
}

/// User-facing reason when a required guest export (the allocation or
/// block-yield hook) is absent or has the wrong signature — the loaded
/// `data/kobako.wasm` does not match the installed gem. Phrased in caller
/// vocabulary: the underlying hook symbol names are not actionable, and
/// the actionable fix is to rebuild the runtime.
const RUNTIME_INCOMPATIBLE: &str =
    "the Sandbox runtime is incompatible; rebuild data/kobako.wasm against the installed version";

/// Resolve the guest's exported linear `memory`. The lookup shape (and its
/// diagnostic) is shared by every Caller-based path here — the write side
/// (`alloc_and_write`), the read side (`read`), and the yield round-trip
/// (`drive_yield`) — so the "no linear memory" reason lives in one place.
/// `read` maps the `Err` to its own `None` outcome via `.ok()`.
fn memory_export(caller: &mut Caller<'_, Invocation>) -> Result<Memory, &'static str> {
    match caller.get_export("memory") {
        Some(Extern::Memory(m)) => Ok(m),
        _ => Err("the loaded Wasm module is not a Kobako-compatible runtime"),
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
            .map_err(|_| RUNTIME_INCOMPATIBLE)?,
        _ => return Err(RUNTIME_INCOMPATIBLE),
    };
    let len = checked_payload_len(bytes.len())?;
    let ptr = alloc
        .call(&mut *caller, len)
        .map_err(|_| "the Sandbox trapped while allocating memory for the request")?;
    if ptr == 0 {
        return Err("the Sandbox ran out of memory while preparing the request");
    }

    let mem = memory_export(caller)?;
    mem.write(&mut *caller, ptr as usize, bytes)
        .map_err(|_| "could not write the request into the Sandbox's memory")?;
    Ok(ptr as u32)
}

/// Copy `[ptr, ptr + len)` out of the guest's linear memory as seen from
/// `caller`. Each failure carries a `&'static str` reason — matching the
/// other Caller-based ops here — so the caller surfaces a specific
/// diagnostic instead of a lumped one; a guest-claimed length past the
/// 16 MiB cap is a wire violation that names the cap (the caller walks
/// the trap path on any `Err`).
pub(super) fn read(
    caller: &mut Caller<'_, Invocation>,
    ptr: i32,
    len: i32,
) -> Result<Vec<u8>, &'static str> {
    let len = usize::try_from(len).map_err(|_| "the Sandbox produced a negative request length")?;
    if len > MAX_DISPATCH_PAYLOAD {
        return Err("request payload exceeds the 16 MiB limit");
    }
    let mem = memory_export(caller)?;
    let data = mem.data(&caller);
    let start =
        usize::try_from(ptr).map_err(|_| "the Sandbox produced a negative request pointer")?;
    let end = start
        .checked_add(len)
        .ok_or("the Sandbox produced an out-of-range request")?;
    data.get(start..end)
        .map(|s| s.to_vec())
        .ok_or("the Sandbox produced an out-of-bounds request")
}

/// Single-dispatch payload cap: 16 MiB in either direction
/// (SPEC.md § Wire Codec; docs/wire-codec.md § ABI). A host↔guest
/// transfer larger than this is a wire violation — the Host Gem walks
/// the trap path rather than allocate or copy the buffer. Held as a
/// constant for now; a future SPEC anchor may let the Host App raise it.
pub(super) const MAX_DISPATCH_PAYLOAD: usize = 16 * 1024 * 1024;

/// Validate a payload length against `MAX_DISPATCH_PAYLOAD` and narrow it
/// to `i32` — the signed wasm ABI width for the guest buffer parameters.
/// Every host *write* boundary (`alloc_and_write`, `drive_yield`,
/// `Runtime::write_envelope`) routes its length through here so the
/// wire-violation reason is uniform; the *read* boundaries compare
/// against `MAX_DISPATCH_PAYLOAD` directly.
pub(super) fn checked_payload_len(len: usize) -> Result<i32, &'static str> {
    if len > MAX_DISPATCH_PAYLOAD {
        return Err("payload exceeds the 16 MiB limit");
    }
    // The cap above sits below `i32::MAX`, so this conversion cannot wrap.
    i32::try_from(len).map_err(|_| "payload exceeds the 16 MiB limit")
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
/// `unpack_u64` in `wasm/kobako-core/src/abi.rs`.
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
    let len_i32 = checked_payload_len(args.len())?;
    let req_ptr = alloc_and_write(caller, args)? as i32;

    let yield_fn = match caller.get_export("__kobako_yield_to_block") {
        Some(Extern::Func(f)) => f
            .typed::<(i32, i32), u64>(&*caller)
            .map_err(|_| RUNTIME_INCOMPATIBLE)?,
        _ => return Err(RUNTIME_INCOMPATIBLE),
    };
    let packed = yield_fn
        .call(&mut *caller, (req_ptr, len_i32))
        .map_err(|_| "the Sandbox trapped while invoking a block")?;
    let (resp_ptr, resp_len) = unpack_outcome_packed(packed);
    if resp_len == 0 {
        return Err("the Sandbox returned an empty block result");
    }
    if resp_len > MAX_DISPATCH_PAYLOAD {
        return Err("block result payload exceeds the 16 MiB limit");
    }

    let mem = memory_export(caller)?;
    let data = mem.data(&caller);
    let range = guest_buffer_range(resp_ptr, resp_len, data.len())
        .map_err(|_| "the Sandbox returned an out-of-bounds block result")?;
    Ok(data[range].to_vec())
}

#[cfg(test)]
mod tests {
    use super::{
        checked_payload_len, guest_buffer_range, unpack_outcome_packed, MAX_DISPATCH_PAYLOAD,
    };

    #[test]
    fn checked_payload_len_accepts_zero_and_the_cap() {
        assert_eq!(checked_payload_len(0), Ok(0));
        assert_eq!(
            checked_payload_len(MAX_DISPATCH_PAYLOAD),
            Ok(MAX_DISPATCH_PAYLOAD as i32)
        );
    }

    #[test]
    fn checked_payload_len_rejects_past_the_cap() {
        assert!(checked_payload_len(MAX_DISPATCH_PAYLOAD + 1).is_err());
        assert!(checked_payload_len(usize::MAX).is_err());
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
