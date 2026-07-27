//! The ABI's fixed values — the numbers and layouts a host and a guest
//! must already agree on before either can read the other's bytes.
//!
//! The exports and the host import that carry the ABI are declared where
//! they are implemented: the guest contract crate emits the exports, the
//! driver resolves them. What lives here is what both sides must spell the
//! same way, and each value is spelled once.
//!
//! [ABI signatures]: ../../../docs/wire-codec.md

/// The Guest ABI version. A host accepts a Guest Binary only when the
/// version it reports equals this one, so a wire change is an increment
/// rather than a negotiation.
pub const ABI_VERSION: u32 = 3;

/// Width in bytes of the length prefix that precedes each invocation-channel
/// frame and the outcome buffer.
pub const FRAME_LEN_SIZE: usize = 4;

/// Single-dispatch size cap: 16 MiB in either direction, applied to the
/// whole envelope rather than the payload alone. A larger transfer is a
/// wire violation the receiving side refuses before allocating for it.
pub const MAX_DISPATCH_PAYLOAD: usize = 16 * 1024 * 1024;

/// Allocation ceiling on a length-prefixed frame. A reader learns a frame's
/// length before it can check anything else about it, so this bounds what a
/// declared length may make it allocate. It sits above
/// `MAX_DISPATCH_PAYLOAD` because it guards the read rather than enforcing
/// the contract: a frame between the two is allocated and then refused on
/// its merits.
pub const MAX_FRAME_LEN: usize = 64 * 1024 * 1024;

// The read guard must admit every frame the contract allows, so a refusal
// names the cap rather than the allocation ceiling.
const _: () = assert!(MAX_FRAME_LEN > MAX_DISPATCH_PAYLOAD);

/// Pack `(ptr, len)` into the u64 the ABI's three buffer-returning
/// functions answer with: the high 32 bits carry the linear-memory pointer,
/// the low 32 the byte length. A `len` of zero is the failure signal.
///
/// ```text
///  63        32 31         0
///  ┌──────────┬────────────┐
///  │   ptr    │    len     │
///  └──────────┴────────────┘
/// ```
#[inline]
pub fn pack_u64(ptr: u32, len: u32) -> u64 {
    ((ptr as u64) << 32) | (len as u64)
}

/// Read a packed u64 back into `(ptr, len)`.
#[inline]
pub fn unpack_u64(packed: u64) -> (u32, u32) {
    ((packed >> 32) as u32, packed as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_layout_is_high_ptr_low_len() {
        let packed = pack_u64(0xAABB_CCDD, 0x1122_3344);
        assert_eq!(
            packed, 0xAABB_CCDD_1122_3344,
            "the packed u64 must carry ptr in the high half and len in the low half"
        );
    }

    #[test]
    fn every_pair_round_trips() {
        for &(ptr, len) in &[
            (0u32, 0u32),
            (0x1000, 1024),
            (0x0001_0000, 4),
            (0x7fff_ffff, 0xffff),
            (1, u32::MAX),
            (u32::MAX, 1),
            (u32::MAX, u32::MAX),
        ] {
            assert_eq!(
                unpack_u64(pack_u64(ptr, len)),
                (ptr, len),
                "a ({ptr:#x}, {len:#x}) pair must survive the pack the ABI returns"
            );
        }
    }
}
