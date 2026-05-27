//! Guest ABI surface — host import + guest exports.
//!
//! This module is the façade for the wasm import/export contract pinned
//! by docs/wire-codec.md § ABI Signatures. The contract is:
//!
//! * **Exactly 1 host import**: `__kobako_dispatch` — the transport bridge the
//!   guest uses to dispatch a Service call to the host. Lives in the
//!   `env` wasm namespace (`(import "env" "__kobako_dispatch" ...)`).
//! * **Exactly 5 guest exports**:
//!   - `__kobako_eval()`                          — reactor entry; runs one-shot user source
//!   - `__kobako_run(env_ptr, env_len)`           — reactor entry; entrypoint dispatch
//!   - `__kobako_alloc(size)`                     — bump/malloc allocator for buffers
//!   - `__kobako_take_outcome()`                  — returns packed (ptr, len) of OUTCOME_BUFFER
//!   - `__kobako_yield_to_block(req_ptr, req_len)` — host-initiated re-entry into a guest block (B-24)
//!
//! The import / export name set is enforced at link time: a guest
//! import the host does not provide traps inside wasmtime, and a
//! missing export fails the `link_func_wrap` lookup on the host side
//! or the `Caller::get_export` lookup inside dispatch. E2E journeys
//! (`test/test_e2e_journeys.rb` + `test/test_sandbox_run.rb`) drive a
//! full host↔guest round-trip against the real `data/kobako.wasm`, so
//! any name drift surfaces before any other test runs.
//!
//! ## Module layout
//!
//! The façade owns the import / export name constants, the
//! `__kobako_dispatch` host import declaration, the WASI reactor
//! `_initialize` shim, and the packed-u64 helpers shared between the
//! host return type and `__kobako_take_outcome`. Each guest export
//! body lives in its own sibling file alongside the helpers it owns:
//!
//! * `eval` — `__kobako_eval` body.
//! * `run` — `__kobako_run` body + invocation-envelope parser.
//! * `boot` — shared mruby boot / preamble install / snippet replay
//!   / pending-exception extraction helpers used by both entry points.
//! * `frames` — stdin frame reader and Frame 1 / Frame 3 decoders.
//! * `outcome_buffer` — `OUTCOME_BUFFER` plus `__kobako_alloc` /
//!   `__kobako_take_outcome` and the Panic / outcome write helpers.
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

#[cfg(target_arch = "wasm32")]
pub(crate) mod block_stack;
#[cfg(any(target_arch = "wasm32", test))]
mod boot;
mod eval;
#[cfg(any(target_arch = "wasm32", test))]
mod frames;
#[cfg(target_arch = "wasm32")]
mod mrb_slot;
mod outcome_buffer;
mod run;
mod yield_block;

pub use eval::__kobako_eval;
pub use outcome_buffer::{__kobako_alloc, __kobako_take_outcome};
pub use run::__kobako_run;
pub use yield_block::__kobako_yield_to_block;

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
    pub fn __kobako_dispatch(req_ptr: u32, req_len: u32) -> u64;
}

// ---------------------------------------------------------------------------
// WASI Reactor `_initialize` entry-point.
// ---------------------------------------------------------------------------

/// WASI Reactor `_initialize` entry-point.
///
/// When compiling as a WASI reactor (`cdylib` targeting
/// `wasm32-wasip1`), the rust-lld linker looks for an `_initialize`
/// export to satisfy the reactor CRT model. Without it the link step
/// fails with:
///
///   rust-lld: error: entry symbol not defined: _initialize
///
/// We export a no-op here because wasi-libc reactor init
/// (`crt1-reactor.o` static ctors) is not required for kobako's boot
/// path — kobako creates and destroys an `mrb_state` inside
/// `__kobako_eval` / `__kobako_run` for every invocation; there are no
/// static C++ constructors or WASI preopen operations that need to run
/// before the first call. Approach (a) from the two known fixes —
/// smaller and sufficient for the kobako use case.
///
/// Per docs/wire-codec.md § ABI Signatures, the four kobako exports
/// counted by the host are `__kobako_eval`, `__kobako_run`,
/// `__kobako_alloc`, and `__kobako_take_outcome`. `_initialize` is
/// WASI reactor bookkeeping and is explicitly excluded from the kobako
/// export count.
#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn _initialize() {
    // No-op: wasi-libc reactor static ctors are not needed for
    // kobako's reactor model. See item-level doc above.
}

// ---------------------------------------------------------------------------
// Packed u64 helpers.
// ---------------------------------------------------------------------------

/// Pack `(ptr, len)` into a single u64: high 32 bits = ptr,
/// low 32 = len. Crate-internal — only the outcome buffer writer in
/// `super::outcome_buffer` and the transport client in `crate::transport::proxy`
/// share this layout with the host. The host callers live behind
/// `#[cfg(target_arch = "wasm32")]`, so on the host target the
/// function exists only for the inline cargo tests below; the lint
/// suppression keeps the dead-code analyser quiet without forcing a
/// wider `pub` than the wasm32 callers need.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
#[inline]
pub(crate) fn pack_u64(ptr: u32, len: u32) -> u64 {
    ((ptr as u64) << 32) | (len as u64)
}

/// Unpack a u64 produced by `pack_u64` back into `(ptr, len)`.
/// Crate-internal companion to `pack_u64`; see that item for the
/// host-target `dead_code` rationale.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
#[inline]
pub(crate) fn unpack_u64(packed: u64) -> (u32, u32) {
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
