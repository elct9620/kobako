//! Guest ABI primitives shared across the wasm boundary.
//!
//! Owns the `__kobako_dispatch` host-import declaration, the
//! packed-u64 helpers pinned by docs/wire-codec.md § ABI Signatures,
//! and the per-invocation outcome-buffer machinery (`alloc` /
//! `take_outcome` / `write_outcome` / `write_panic`). The
//! `#[no_mangle]` guest exports themselves are emitted by
//! `crate::export_guest!` in the leaf shell crate that links the
//! final Guest Binary; this module carries only the building blocks
//! they delegate to.
//!
//! ## Packed u64 layout
//!
//! Both `__kobako_dispatch` (host import) and `__kobako_take_outcome`
//! (guest export) return a u64 (i64 at the wasm type level) carrying
//! two u32 values: the high 32 bits are the wasm linear memory ptr,
//! the low 32 bits are the byte length.
//!
//! ```text
//!  63        32 31         0
//!  ┌──────────┬────────────┐
//!  │   ptr    │    len     │
//!  └──────────┴────────────┘
//!  high 32 bits  low 32 bits
//! ```
//!
//! Extraction: `ptr = (packed >> 32) as u32; len = packed as u32`.
//! Composition: `(ptr as u64) << 32 | len as u64`.
//! `len == 0` is a wire violation (host walks trap path).

mod outcome_buffer;

pub use outcome_buffer::{alloc, take_outcome, write_outcome, write_panic};

/// The Guest ABI version this crate implements, reported through the
/// `__kobako_abi_version` export `crate::export_guest!` emits. The host
/// accepts a Guest Binary only on equality (docs/wire-codec.md § ABI
/// Version; docs/behavior.md B-40 / E-42).
pub const ABI_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// Host import declaration.
// ---------------------------------------------------------------------------
//
// The `wasm_import_module = "env"` attribute pins the import namespace.
// Signature: `(req_ptr: i32, req_len: i32) -> i64` per docs/wire-codec.md
// § ABI Signatures. We only declare the import on the wasm32 target —
// on the host target (where rlib codec tests run) there is no host to
// provide the symbol.
#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "env")]
extern "C" {
    /// Host-provided transport bridge. Guest writes a Request payload at
    /// `[req_ptr, req_ptr + req_len)` and calls this; host returns a
    /// packed u64 holding (response_ptr, response_len) of a buffer the
    /// host allocated via `__kobako_alloc` inside the same call frame.
    /// Crate-internal — guests dispatch through `transport::proxy`,
    /// never the raw import.
    pub(crate) fn __kobako_dispatch(req_ptr: u32, req_len: u32) -> u64;
}

// ---------------------------------------------------------------------------
// Packed u64 helpers.
// ---------------------------------------------------------------------------

/// Pack `(ptr, len)` into a single u64: high 32 bits = ptr,
/// low 32 = len. The outcome-buffer writer and the transport client
/// share this layout with the host (docs/wire-codec.md § ABI
/// Signatures).
#[inline]
pub fn pack_u64(ptr: u32, len: u32) -> u64 {
    ((ptr as u64) << 32) | (len as u64)
}

/// Unpack a u64 produced by `pack_u64` back into `(ptr, len)`.
#[inline]
pub fn unpack_u64(packed: u64) -> (u32, u32) {
    let ptr = (packed >> 32) as u32;
    let len = packed as u32;
    (ptr, len)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_unpack_roundtrip_zero() {
        let packed = pack_u64(0, 0);
        assert_eq!(packed, 0);
        assert_eq!(unpack_u64(packed), (0, 0));
    }

    #[test]
    fn pack_unpack_roundtrip_max() {
        let packed = pack_u64(u32::MAX, u32::MAX);
        assert_eq!(packed, u64::MAX);
        assert_eq!(unpack_u64(packed), (u32::MAX, u32::MAX));
    }

    #[test]
    fn pack_unpack_roundtrip_common() {
        // Representative common cases: small ptr + 1 KiB len,
        // page-sized ptr + small len, midrange both.
        for &(ptr, len) in &[
            (0x1000_u32, 1024_u32),
            (0x0001_0000, 4),
            (0x7fff_ffff, 0xffff),
            (1, u32::MAX),
            (u32::MAX, 1),
        ] {
            let packed = pack_u64(ptr, len);
            assert_eq!(
                unpack_u64(packed),
                (ptr, len),
                "roundtrip failed for ({ptr:#x}, {len:#x})"
            );
        }
    }

    #[test]
    fn pack_layout_is_high_ptr_low_len() {
        // docs/wire-codec.md § ABI Signatures pins the bit layout:
        // high 32 = ptr, low 32 = len. Verify with a known-distinct
        // ptr / len pair.
        let packed = pack_u64(0xAABB_CCDD, 0x1122_3344);
        assert_eq!(packed, 0xAABB_CCDD_1122_3344);
        assert_eq!((packed >> 32) as u32, 0xAABB_CCDD);
        assert_eq!(packed as u32, 0x1122_3344);
    }
}
