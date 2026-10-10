//! Guest linear-memory I/O for every host↔guest buffer handoff, whether
//! the invocation body drives it through its `Store` or a host import
//! drives it through its `Caller`.
//!
//! The two shared steps answer an error kind, and each caller words it,
//! because depth decides what the same problem means: before the export
//! call it is a `Trap` or an intact runtime, inside a callback it is a
//! wire failure the guest receives.

use wasmtime::{AsContext, AsContextMut, Caller, Memory};

use crate::exports::{Exports, RUNTIME_INCOMPATIBLE};
use crate::invocation::Invocation;
use kobako_transport::abi::{unpack_ptr_len, MAX_DISPATCH_PAYLOAD};

// The size-limit messages here and in `frames` name the cap as 16 MiB, so
// moving the cap fails the build until they are reworded with it.
const _: () = assert!(MAX_DISPATCH_PAYLOAD == 16 << 20);

/// User-facing message for the "the loaded Wasm module is not a
/// Kobako-shaped runtime at all" failure mode, such as an instance with no
/// `memory` export.
pub(crate) const SANDBOX_RUNTIME_NOT_KOBAKO: &str =
    "the loaded Wasm module is not a Kobako-compatible runtime";

/// Why a host write into guest memory failed.
pub(crate) enum WriteError {
    TooLarge,
    /// The allocator or the memory is missing or mistyped; carries the
    /// message for it.
    Incompatible(&'static str),
    Trapped(wasmtime::Error),
    /// The allocator ran and answered 0.
    OutOfMemory,
    OutOfBounds,
}

/// Why a host read of a guest-named buffer failed.
pub(crate) enum ReadError {
    TooLarge,
    OutOfBounds,
}

/// Allocate a guest buffer through `__kobako_alloc` and copy `bytes` into
/// it, answering the buffer's address.
pub(crate) fn alloc_and_write(
    mut ctx: impl AsContextMut,
    exports: &Exports,
    bytes: &[u8],
) -> Result<u32, WriteError> {
    let len = checked_payload_len(bytes.len()).map_err(|_| WriteError::TooLarge)?;
    let alloc = exports
        .alloc
        .as_ref()
        .ok_or(WriteError::Incompatible(RUNTIME_INCOMPATIBLE))?;
    let memory = exports
        .memory
        .ok_or(WriteError::Incompatible(SANDBOX_RUNTIME_NOT_KOBAKO))?;
    let ptr = alloc
        .call(&mut ctx, len as u32)
        .map_err(WriteError::Trapped)?;
    if ptr == 0 {
        return Err(WriteError::OutOfMemory);
    }
    memory
        .write(&mut ctx, ptr as usize, bytes)
        .map_err(|_| WriteError::OutOfBounds)?;
    Ok(ptr)
}

/// Copy out the buffer the guest names; a length past the 16 MiB cap is
/// refused before memory is touched.
pub(crate) fn read_buffer(
    ctx: impl AsContext,
    memory: Memory,
    ptr: usize,
    len: usize,
) -> Result<Vec<u8>, ReadError> {
    if len > MAX_DISPATCH_PAYLOAD {
        return Err(ReadError::TooLarge);
    }
    let data = memory.data(&ctx);
    let range = guest_buffer_range(ptr, len, data.len()).map_err(|_| ReadError::OutOfBounds)?;
    Ok(data[range].to_vec())
}

/// The handles of the instance a host import runs inside.
fn callback_exports(caller: &Caller<'_, Invocation>) -> Result<Exports, &'static str> {
    caller.data().exports().ok_or(SANDBOX_RUNTIME_NOT_KOBAKO)
}

/// Keep a trap the guest raised during a callback into it for the dispatch
/// import to end the invocation with, and answer the callback's own reason.
fn trapped(
    caller: &mut Caller<'_, Invocation>,
    trap: wasmtime::Error,
    reason: &'static str,
) -> &'static str {
    caller.data_mut().record_reentry_trap(trap);
    reason
}

/// The Call the guest hands the dispatch import. Its `(ptr, len)` are
/// guest addresses, so they are read unsigned.
pub(crate) fn read_request(
    caller: &mut Caller<'_, Invocation>,
    req_ptr: i32,
    req_len: i32,
) -> Result<Vec<u8>, &'static str> {
    let memory = callback_exports(caller)?
        .memory
        .ok_or(SANDBOX_RUNTIME_NOT_KOBAKO)?;
    read_buffer(
        &*caller,
        memory,
        req_ptr as u32 as usize,
        req_len as u32 as usize,
    )
    .map_err(|err| match err {
        ReadError::TooLarge => "request payload exceeds the 16 MiB limit",
        ReadError::OutOfBounds => "the Sandbox produced an out-of-bounds request",
    })
}

/// Write `bytes` into the guest from inside a callback. A guest that has
/// already trapped is not called again.
pub(crate) fn write_from_callback(
    caller: &mut Caller<'_, Invocation>,
    bytes: &[u8],
) -> Result<u32, &'static str> {
    if caller.data().reentry_trapped() {
        return Err("the Sandbox already trapped during this invocation");
    }
    let exports = callback_exports(caller)?;
    alloc_and_write(&mut *caller, &exports, bytes).map_err(|err| match err {
        WriteError::TooLarge => "payload exceeds the 16 MiB limit",
        WriteError::Incompatible(message) => message,
        WriteError::Trapped(trap) => trapped(
            caller,
            trap,
            "the Sandbox trapped while allocating memory for the request",
        ),
        WriteError::OutOfMemory => "the Sandbox ran out of memory while preparing the request",
        WriteError::OutOfBounds => "could not write the request into the Sandbox's memory",
    })
}

/// Every host write boundary routes its length through here, so the
/// wire-violation reason is uniform.
pub(crate) fn checked_payload_len(len: usize) -> Result<i32, &'static str> {
    if len > MAX_DISPATCH_PAYLOAD {
        return Err("payload exceeds the 16 MiB limit");
    }
    // The cap above sits below `i32::MAX`, so this conversion cannot wrap.
    i32::try_from(len).map_err(|_| "payload exceeds the 16 MiB limit")
}

pub(crate) fn guest_buffer_range(
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

/// The `(ptr, len)` the guest packs for any buffer it hands back, the
/// outcome and a block's result alike.
pub(crate) fn unpack_guest_buffer(packed: u64) -> (usize, usize) {
    let (ptr, len) = unpack_ptr_len(packed);
    (ptr as usize, len as usize)
}

pub(crate) fn drive_yield(
    caller: &mut Caller<'_, Invocation>,
    args: &[u8],
) -> Result<Vec<u8>, &'static str> {
    let len_i32 = checked_payload_len(args.len())?;
    let req_ptr = write_from_callback(caller, args)? as i32;

    let exports = callback_exports(caller)?;
    let yield_fn = exports.yield_to_block.ok_or(RUNTIME_INCOMPATIBLE)?;
    let memory = exports.memory.ok_or(SANDBOX_RUNTIME_NOT_KOBAKO)?;
    let packed = yield_fn
        .call(&mut *caller, (req_ptr, len_i32))
        .map_err(|trap| trapped(caller, trap, "the Sandbox trapped while invoking a block"))?;
    let (reply_ptr, reply_len) = unpack_guest_buffer(packed);
    if reply_len == 0 {
        return Err("the Sandbox returned an empty block result");
    }
    read_buffer(&*caller, memory, reply_ptr, reply_len).map_err(|err| match err {
        ReadError::TooLarge => "block result payload exceeds the 16 MiB limit",
        ReadError::OutOfBounds => "the Sandbox returned an out-of-bounds block result",
    })
}

#[cfg(test)]
mod tests {
    use super::{
        checked_payload_len, guest_buffer_range, unpack_guest_buffer, MAX_DISPATCH_PAYLOAD,
    };

    // @behavior WE-059
    #[test]
    fn checked_payload_len_accepts_zero_and_the_cap() {
        assert_eq!(checked_payload_len(0), Ok(0));
        assert_eq!(
            checked_payload_len(MAX_DISPATCH_PAYLOAD),
            Ok(MAX_DISPATCH_PAYLOAD as i32)
        );
    }

    // @behavior WE-060
    #[test]
    fn checked_payload_len_rejects_past_the_cap() {
        assert!(checked_payload_len(MAX_DISPATCH_PAYLOAD + 1).is_err());
        assert!(checked_payload_len(usize::MAX).is_err());
    }

    // @behavior WE-061
    #[test]
    fn guest_buffer_range_returns_half_open_range() {
        assert_eq!(guest_buffer_range(10, 5, 100), Ok(10..15));
    }

    // @behavior WE-062
    #[test]
    fn guest_buffer_range_accepts_zero_length_at_any_in_bounds_ptr() {
        assert_eq!(guest_buffer_range(0, 0, 0), Ok(0..0));
        assert_eq!(guest_buffer_range(42, 0, 100), Ok(42..42));
    }

    // @behavior WE-063
    #[test]
    fn guest_buffer_range_rejects_ptr_plus_len_overflow() {
        assert!(guest_buffer_range(usize::MAX, 1, usize::MAX).is_err());
    }

    // @behavior WE-064
    #[test]
    fn guest_buffer_range_rejects_end_past_memory() {
        assert!(guest_buffer_range(10, 100, 50).is_err());
        assert_eq!(guest_buffer_range(0, 50, 50), Ok(0..50));
    }

    // @behavior WE-001
    #[test]
    fn unpack_guest_buffer_extracts_high_ptr_low_len() {
        assert_eq!(
            unpack_guest_buffer(0xAABB_CCDD_1122_3344),
            (0xAABB_CCDD, 0x1122_3344)
        );
    }

    // @behavior WE-065
    #[test]
    fn unpack_guest_buffer_zero_decodes_to_zero_pair() {
        assert_eq!(unpack_guest_buffer(0), (0, 0));
    }
}
