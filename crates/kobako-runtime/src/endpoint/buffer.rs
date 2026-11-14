//! Guest linear-memory I/O for the ABI's list handoffs, whether the
//! invocation body or an upcall drives one.
//!
//! Every list the ABI carries is named by a return area: two little-endian
//! u32, its place then its length, 4-byte aligned, as the Canonical ABI lays
//! a `list<u8>` out. The shared steps answer an error kind, and each caller
//! words it, because depth decides what the same problem means: before the
//! export call it is a `Trap`, inside a callback it is a wire failure the
//! guest receives.

use kobako_transport::abi::MAX_MESSAGE_LEN;

use super::{GuestInstance, RUNTIME_INCOMPATIBLE, SANDBOX_RUNTIME_NOT_KOBAKO};

// The size-limit messages here and in the endpoint name the cap as 16 MiB, so
// moving the cap fails the build until they are reworded with it.
const _: () = assert!(MAX_MESSAGE_LEN == 16 << 20);

/// The bytes of a return area: a u32 place, then a u32 length.
const RETURN_AREA_SIZE: usize = 8;

/// The alignment a return area keeps, its u32's own.
const RETURN_AREA_ALIGN: usize = 4;

/// Why a host write into guest memory failed.
pub(super) enum WriteError<F> {
    TooLarge,
    /// The allocator or the memory is missing or mistyped; carries the
    /// message for it.
    Incompatible(&'static str),
    Trapped(F),
    OutOfBounds,
}

/// Why a host read of a guest-named buffer failed.
pub(super) enum ReadError {
    TooLarge,
    OutOfBounds,
}

/// Take room from the guest's `cabi_realloc` and copy `bytes` into it,
/// answering the room's address.
pub(super) fn alloc_and_write<G: GuestInstance>(
    guest: &mut G,
    bytes: &[u8],
) -> Result<u32, WriteError<G::Fault>> {
    let len = checked_payload_len(bytes.len()).map_err(|_| WriteError::TooLarge)?;
    if guest.memory().is_none() {
        return Err(WriteError::Incompatible(SANDBOX_RUNTIME_NOT_KOBAKO));
    }
    let ptr = guest
        .realloc(len as u32)
        .ok_or(WriteError::Incompatible(RUNTIME_INCOMPATIBLE))?
        .map_err(WriteError::Trapped)?;
    let memory = guest
        .memory()
        .ok_or(WriteError::Incompatible(SANDBOX_RUNTIME_NOT_KOBAKO))?;
    let range = guest_buffer_range(ptr as usize, bytes.len(), memory.len())
        .map_err(|_| WriteError::OutOfBounds)?;
    memory[range].copy_from_slice(bytes);
    Ok(ptr)
}

/// Write `bytes` into the guest and name them in the return area at `ret`.
/// A return area the guest cannot hold is refused before the guest gives
/// room for the answer; memory only grows, so it still holds after.
pub(super) fn lower_list<G: GuestInstance>(
    guest: &mut G,
    bytes: &[u8],
    ret: u32,
) -> Result<(), WriteError<G::Fault>> {
    let mem_size = guest
        .memory()
        .ok_or(WriteError::Incompatible(SANDBOX_RUNTIME_NOT_KOBAKO))?
        .len();
    let area = return_area_range(ret as usize, mem_size).map_err(|_| WriteError::OutOfBounds)?;
    let ptr = alloc_and_write(guest, bytes)?;
    let memory = guest
        .memory()
        .ok_or(WriteError::Incompatible(SANDBOX_RUNTIME_NOT_KOBAKO))?;
    // The write has already held the length to the payload cap.
    memory[area.start..area.start + 4].copy_from_slice(&ptr.to_le_bytes());
    memory[area.start + 4..area.end].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
    Ok(())
}

/// Copy out the list the return area at `ret` names; a length past the
/// 16 MiB cap is refused before the list is touched.
pub(super) fn lift_list(memory: &[u8], ret: u32) -> Result<Vec<u8>, ReadError> {
    let area = return_area_range(ret as usize, memory.len()).map_err(|_| ReadError::OutOfBounds)?;
    let word =
        |at: usize| u32::from_le_bytes(memory[at..at + 4].try_into().expect("a u32's width"));
    let (ptr, len) = (word(area.start), word(area.start + 4));
    read_buffer(memory, ptr as usize, len as usize)
}

/// Copy out the buffer the guest names; a length past the 16 MiB cap is
/// refused before memory is touched.
pub(super) fn read_buffer(data: &[u8], ptr: usize, len: usize) -> Result<Vec<u8>, ReadError> {
    if len > MAX_MESSAGE_LEN {
        return Err(ReadError::TooLarge);
    }
    let range = guest_buffer_range(ptr, len, data.len()).map_err(|_| ReadError::OutOfBounds)?;
    Ok(data[range].to_vec())
}

/// Every host write boundary routes its length through here, so the
/// wire-violation reason is uniform.
pub(super) fn checked_payload_len(len: usize) -> Result<i32, &'static str> {
    if len > MAX_MESSAGE_LEN {
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

/// The bytes of guest memory the return area at `ret` covers, refused off
/// its alignment or reaching past memory.
fn return_area_range(ret: usize, mem_size: usize) -> Result<core::ops::Range<usize>, &'static str> {
    if !ret.is_multiple_of(RETURN_AREA_ALIGN) {
        return Err("the return area is not aligned");
    }
    guest_buffer_range(ret, RETURN_AREA_SIZE, mem_size)
}

#[cfg(test)]
mod tests {
    use super::{checked_payload_len, guest_buffer_range, lift_list, lower_list, MAX_MESSAGE_LEN};
    use crate::endpoint::{Calls, GuestInstance};
    use crate::error::Trap;

    /// 64 bytes of guest memory whose allocator counts the room it gave.
    #[derive(Default)]
    struct Counted {
        memory: Vec<u8>,
        given: u32,
        calls: Calls<String>,
    }

    impl GuestInstance for Counted {
        type Fault = String;

        fn calls(&mut self) -> &mut Calls<String> {
            &mut self.calls
        }

        fn memory(&mut self) -> Option<&mut [u8]> {
            Some(&mut self.memory)
        }

        fn realloc(&mut self, _len: u32) -> Option<Result<u32, String>> {
            self.given += 1;
            Some(Ok(32))
        }

        fn downcall(&mut self, _ptr: i32, _len: i32) -> Option<Result<u32, String>> {
            None
        }

        fn cap_trap_of(_fault: &String) -> Option<Trap> {
            None
        }
    }

    // @behavior WE-059
    #[test]
    fn a_transfer_of_nothing_or_at_the_cap_is_admitted() {
        assert_eq!(
            (checked_payload_len(0), checked_payload_len(MAX_MESSAGE_LEN)),
            (Ok(0), Ok(MAX_MESSAGE_LEN as i32)),
            "0 bytes and MAX_MESSAGE_LEN bytes through checked_payload_len must be admitted"
        );
    }

    // @behavior WE-060
    #[test]
    fn a_transfer_past_the_cap_is_refused() {
        assert!(
            checked_payload_len(MAX_MESSAGE_LEN + 1).is_err()
                && checked_payload_len(usize::MAX).is_err(),
            "one byte past MAX_MESSAGE_LEN and usize::MAX through checked_payload_len must be \
             refused"
        );
    }

    // @behavior WE-061
    #[test]
    fn a_guest_buffer_is_read_half_open() {
        assert_eq!(
            guest_buffer_range(10, 5, 100),
            Ok(10..15),
            "5 bytes at 10 in 100 bytes of memory through guest_buffer_range must answer 10..15"
        );
    }

    // @behavior WE-062
    #[test]
    fn a_buffer_of_no_length_is_read_at_any_pointer_inside_memory() {
        assert_eq!(
            (guest_buffer_range(0, 0, 0), guest_buffer_range(42, 0, 100)),
            (Ok(0..0), Ok(42..42)),
            "no bytes at 0 in no memory and at 42 in 100 bytes through guest_buffer_range must \
             answer an empty range there"
        );
    }

    // @behavior WE-063
    #[test]
    fn a_pointer_and_length_that_overflow_are_refused() {
        assert!(
            guest_buffer_range(usize::MAX, 1, usize::MAX).is_err(),
            "1 byte at usize::MAX through guest_buffer_range must be refused"
        );
    }

    // @behavior WE-064
    #[test]
    fn a_buffer_ending_past_memory_is_refused() {
        assert_eq!(
            (guest_buffer_range(10, 100, 50).is_err(), guest_buffer_range(0, 50, 50)),
            (true, Ok(0..50)),
            "100 bytes at 10 in 50 bytes of memory through guest_buffer_range must be refused, and \
             50 bytes at 0 admitted"
        );
    }

    /// 64 bytes of memory holding `answer` at 32, named by a return area at 8.
    fn memory_naming(answer: &[u8]) -> Vec<u8> {
        let mut memory = vec![0; 64];
        memory[8..12].copy_from_slice(&32u32.to_le_bytes());
        memory[12..16].copy_from_slice(&(answer.len() as u32).to_le_bytes());
        memory[32..32 + answer.len()].copy_from_slice(answer);
        memory
    }

    // @behavior WE-092
    #[test]
    fn a_return_area_names_its_answers_place_then_its_length() {
        let memory = memory_naming(b"answer");

        assert_eq!(
            lift_list(&memory, 8).ok(),
            Some(b"answer".to_vec()),
            "a return area holding 32 then 6, read through lift_list, must answer the 6 bytes \
             at 32"
        );
    }

    // @behavior WE-093
    #[test]
    fn a_return_area_off_its_alignment_or_past_memory_is_refused() {
        let mut memory = memory_naming(b"answer");
        // At 1, off alignment, the same place and length the area at 8 holds.
        memory.copy_within(8..16, 1);

        assert!(
            [1, 60, u32::MAX - 3]
                .iter()
                .all(|&ret| lift_list(&memory, ret).is_err()),
            "a return area at 1 (unaligned, naming a list inside memory), 60 (past 64 bytes), or \
             u32::MAX - 3 through lift_list must be refused"
        );
    }

    // @behavior WE-093
    #[test]
    fn an_answer_for_a_return_area_the_guest_cannot_hold_is_never_copied() {
        let mut guest = Counted {
            memory: vec![0; 64],
            ..Counted::default()
        };
        let refused = [1, 60, u32::MAX - 3]
            .iter()
            .all(|&ret| lower_list(&mut guest, b"answer", ret).is_err());

        assert_eq!(
            (
                refused,
                guest.given,
                guest.memory.iter().all(|&byte| byte == 0)
            ),
            (true, 0, true),
            "an answer written for a return area at 1 (unaligned), 60 (past 64 bytes), or \
             u32::MAX - 3 must be refused before the guest's allocator gives it room or any byte \
             lands in guest memory"
        );
    }
}
