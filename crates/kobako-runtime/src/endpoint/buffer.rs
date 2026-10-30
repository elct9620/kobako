//! Guest linear-memory I/O for the ABI's buffer handoffs, whether the
//! invocation body or an upcall drives one.
//!
//! The two shared steps answer an error kind, and each caller words it,
//! because depth decides what the same problem means: before the export
//! call it is a `Trap` or an intact runtime, inside a callback it is a
//! wire failure the guest receives.

use kobako_transport::abi::{unpack_ptr_len, MAX_DISPATCH_PAYLOAD};

use super::{GuestInstance, RUNTIME_INCOMPATIBLE, SANDBOX_RUNTIME_NOT_KOBAKO};

// The size-limit messages here and in the endpoint name the cap as 16 MiB, so
// moving the cap fails the build until they are reworded with it.
const _: () = assert!(MAX_DISPATCH_PAYLOAD == 16 << 20);

/// Why a host write into guest memory failed.
pub(super) enum WriteError<F> {
    TooLarge,
    /// The allocator or the memory is missing or mistyped; carries the
    /// message for it.
    Incompatible(&'static str),
    Trapped(F),
    /// The allocator ran and answered 0.
    OutOfMemory,
    OutOfBounds,
}

/// Why a host read of a guest-named buffer failed.
pub(super) enum ReadError {
    TooLarge,
    OutOfBounds,
}

/// Allocate a guest buffer and copy `bytes` into it, answering the buffer's
/// address.
pub(super) fn alloc_and_write<G: GuestInstance>(
    guest: &mut G,
    bytes: &[u8],
) -> Result<u32, WriteError<G::Fault>> {
    let len = checked_payload_len(bytes.len()).map_err(|_| WriteError::TooLarge)?;
    if guest.memory().is_none() {
        return Err(WriteError::Incompatible(SANDBOX_RUNTIME_NOT_KOBAKO));
    }
    let ptr = guest
        .alloc(len as u32)
        .ok_or(WriteError::Incompatible(RUNTIME_INCOMPATIBLE))?
        .map_err(WriteError::Trapped)?;
    if ptr == 0 {
        return Err(WriteError::OutOfMemory);
    }
    let memory = guest
        .memory()
        .ok_or(WriteError::Incompatible(SANDBOX_RUNTIME_NOT_KOBAKO))?;
    let range = guest_buffer_range(ptr as usize, bytes.len(), memory.len())
        .map_err(|_| WriteError::OutOfBounds)?;
    memory[range].copy_from_slice(bytes);
    Ok(ptr)
}

/// Copy out the buffer the guest names; a length past the 16 MiB cap is
/// refused before memory is touched.
pub(super) fn read_buffer(data: &[u8], ptr: usize, len: usize) -> Result<Vec<u8>, ReadError> {
    if len > MAX_DISPATCH_PAYLOAD {
        return Err(ReadError::TooLarge);
    }
    let range = guest_buffer_range(ptr, len, data.len()).map_err(|_| ReadError::OutOfBounds)?;
    Ok(data[range].to_vec())
}

/// Every host write boundary routes its length through here, so the
/// wire-violation reason is uniform.
pub(super) fn checked_payload_len(len: usize) -> Result<i32, &'static str> {
    if len > MAX_DISPATCH_PAYLOAD {
        return Err("payload exceeds the 16 MiB limit");
    }
    // The cap above sits below `i32::MAX`, so this conversion cannot wrap.
    i32::try_from(len).map_err(|_| "payload exceeds the 16 MiB limit")
}

/// The bytes of guest memory a `(ptr, len)` names.
fn guest_buffer_range(
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

/// The `(ptr, len)` the guest packs for any answer it hands back, the
/// outcome and a block's result alike.
pub(super) fn unpack_guest_buffer(packed: u64) -> (usize, usize) {
    let (ptr, len) = unpack_ptr_len(packed);
    (ptr as usize, len as usize)
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
