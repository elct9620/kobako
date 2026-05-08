//! Wire ABI surface — host import + guest exports.
//!
//! This module declares the wasm import/export contract pinned by SPEC.md
//! "ABI Signatures". The contract is:
//!
//! * **Exactly 1 host import**: `__kobako_rpc_call` — the RPC bridge guest
//!   uses to dispatch a Service call to the host. Lives in the `env`
//!   wasm namespace (`(import "env" "__kobako_rpc_call" ...)`).
//! * **Exactly 3 guest exports**:
//!   - `__kobako_run`             — reactor entry; runs boot script
//!   - `__kobako_alloc(size)`     — bump/malloc allocator for buffers
//!   - `__kobako_take_outcome()`  — returns packed (ptr, len) of OUTCOME_BUFFER
//!
//! This item delivers the **ABI shape** only. Bodies are stubs marked
//! `unimplemented!()`; later items (#10 boot script, #11 allocator, #12 host
//! linker) fill them in. The build-pipeline guard (item #26) inspects the
//! emitted wasm and verifies exactly these names appear.
//!
//! ## Packed u64 layout
//!
//! Both `__kobako_rpc_call` (host import) and `__kobako_take_outcome`
//! (guest export) return a u64 (i64 at the wasm type level) carrying two
//! u32 values: the high 32 bits are the wasm linear memory ptr, the low 32
//! bits are the byte length.
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

/// Wasm namespace the host import lives in (`env`, per SPEC.md "ABI
/// Signatures").
pub const IMPORT_MODULE: &str = "env";

/// Sole host-provided import function name.
pub const IMPORT_NAME: &str = "__kobako_rpc_call";

/// All three guest-provided export names, in declaration order.
pub const EXPORT_NAMES: [&str; 3] = [
    "__kobako_run",
    "__kobako_alloc",
    "__kobako_take_outcome",
];

// ---------------------------------------------------------------------------
// Host import declaration.
// ---------------------------------------------------------------------------
//
// The `wasm_import_module = "env"` attribute pins the import namespace.
// Signature: `(req_ptr: i32, req_len: i32) -> i64` per SPEC ABI Signatures.
// We only declare the import on the wasm32 target — on the host target
// (where rlib codec tests run) there is no host to provide the symbol.
#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "env")]
extern "C" {
    /// Host-provided RPC bridge. Guest writes a Request payload at
    /// `[req_ptr, req_ptr + req_len)` and calls this; host returns a packed
    /// u64 holding (response_ptr, response_len) of a buffer the host
    /// allocated via `__kobako_alloc` inside the same call frame.
    pub fn __kobako_rpc_call(req_ptr: u32, req_len: u32) -> u64;
}

// ---------------------------------------------------------------------------
// Guest exports.
// ---------------------------------------------------------------------------
//
// Signatures must match the SPEC table. Bodies are deliberate stubs — item
// #9 delivers the symbol shape so the build-pipeline guard (item #26) can
// run; later items wire real bodies in.

/// Reactor entry — runs the three-job boot script, writing the outcome
/// envelope to OUTCOME_BUFFER before returning. Signature: `() -> ()`.
///
/// Stub: real implementation arrives with item #10 (boot script). The
/// `wasm32` arm references `__kobako_rpc_call` behind a never-taken branch
/// so the linker preserves the import in the final wasm artifact (the
/// invariant guard parses imports/exports from the wasm binary; an unused
/// import would be dead-code-stripped). Item #10 will replace this with
/// the real call site.
#[no_mangle]
pub extern "C" fn __kobako_run() {
    #[cfg(target_arch = "wasm32")]
    unsafe {
        // Volatile read so the optimizer cannot fold the branch away.
        // `KEEP_ALIVE` is always `false`, so the call never executes.
        if core::ptr::read_volatile(&KEEP_ALIVE) {
            let _ = __kobako_rpc_call(0, 0);
        }
    }
    unimplemented!("__kobako_run body lands with item #10 (boot script)")
}

#[cfg(target_arch = "wasm32")]
static KEEP_ALIVE: bool = false;

/// Guest allocator — hands out a `size`-byte buffer in wasm linear memory
/// and returns its ptr (u32). Returns 0 on allocation failure (host treats
/// 0 as a trap signal). Signature: `(size: i32) -> i32`.
///
/// Stub: real implementation arrives with item #11 (allocator).
#[no_mangle]
pub extern "C" fn __kobako_alloc(_size: u32) -> u32 {
    unimplemented!("__kobako_alloc body lands with item #11 (allocator)")
}

/// Outcome reader — host calls this after `__kobako_run` returns to fetch
/// the OUTCOME_BUFFER bytes. Returns packed u64 `(ptr << 32) | len`.
/// `len == 0` is a wire violation. Signature: `() -> i64`.
///
/// Stub: real implementation arrives with item #10 (boot script populates
/// OUTCOME_BUFFER; this export hands its ptr/len back to the host).
#[no_mangle]
pub extern "C" fn __kobako_take_outcome() -> u64 {
    unimplemented!("__kobako_take_outcome body lands with item #10 (boot script)")
}

// ---------------------------------------------------------------------------
// Packed u64 helpers.
// ---------------------------------------------------------------------------

/// Pack `(ptr, len)` into a single u64: high 32 bits = ptr, low 32 = len.
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
    fn import_module_name_is_env() {
        // SPEC pins host import to the `env` namespace. Changing this
        // is a wire-breaking change.
        assert_eq!(IMPORT_MODULE, "env");
    }

    #[test]
    fn import_name_matches_spec() {
        assert_eq!(IMPORT_NAME, "__kobako_rpc_call");
    }

    #[test]
    fn export_names_match_spec() {
        assert_eq!(
            EXPORT_NAMES,
            ["__kobako_run", "__kobako_alloc", "__kobako_take_outcome"],
        );
    }

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
        // Representative common cases: small ptr + 1 KiB len, page-sized
        // ptr + small len, midrange both.
        for &(ptr, len) in &[
            (0x1000_u32, 1024_u32),
            (0x0001_0000, 4),
            (0x7fff_ffff, 0xffff),
            (1, u32::MAX),
            (u32::MAX, 1),
        ] {
            let packed = pack_u64(ptr, len);
            assert_eq!(unpack_u64(packed), (ptr, len), "roundtrip failed for ({ptr:#x}, {len:#x})");
        }
    }

    #[test]
    fn pack_layout_is_high_ptr_low_len() {
        // SPEC ABI Signatures pins the bit layout: high 32 = ptr, low 32 = len.
        // Verify with a known-distinct ptr / len pair.
        let packed = pack_u64(0xAABB_CCDD, 0x1122_3344);
        assert_eq!(packed, 0xAABB_CCDD_1122_3344);
        assert_eq!((packed >> 32) as u32, 0xAABB_CCDD);
        assert_eq!(packed as u32, 0x1122_3344);
    }
}
