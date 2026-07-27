//! Guest ABI machinery: the `__kobako_dispatch` host-import declaration
//! and the per-invocation outcome-buffer (`alloc` / `take_outcome` /
//! `write_outcome` / `write_panic`).
//!
//! The values the ABI fixes — the version, the packed-return layout —
//! are `kobako_transport::abi`'s and are re-exported here so
//! `crate::export_guest!` reaches them through `$crate` in the shell that
//! expands it. The `#[no_mangle]` exports themselves are that macro's;
//! this module carries only what they delegate to.

mod outcome_buffer;

pub use kobako_transport::abi::{pack_u64, unpack_u64, ABI_VERSION};
pub use outcome_buffer::{alloc, take_outcome, write_outcome, write_panic};

// ---------------------------------------------------------------------------
// Host import declaration.
// ---------------------------------------------------------------------------
//
// The `wasm_import_module = "env"` attribute pins the import namespace.
// Signature: `(req_ptr: i32, req_len: i32) -> i64` per docs/wire-codec.md
// § ABI Signatures. We only declare the import on the wasm32 target —
// on the host target (where the rlib's unit tests run) there is no
// host to provide the symbol.
#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "env")]
extern "C" {
    /// Host-provided transport bridge. Guest writes a Call payload at
    /// `[req_ptr, req_ptr + req_len)` and calls this; host returns a
    /// packed u64 holding (response_ptr, response_len) of a buffer the
    /// host allocated via `__kobako_alloc` inside the same call frame.
    /// Crate-internal — guests dispatch through `transport::proxy`,
    /// never the raw import.
    pub(crate) fn __kobako_dispatch(req_ptr: u32, req_len: u32) -> u64;
}
