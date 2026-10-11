//! Guest ABI machinery: the `__kobako_upcall` host-import declaration,
//! the guest memory the host writes into (`alloc`), and the downcall a
//! guest answers (`serve`, framed out through `answer`).
//!
//! The values the ABI fixes — the version, the packed-return layout —
//! are `kobako_transport::abi`'s and are re-exported here so
//! `crate::export_guest!` reaches them through `$crate` in the shell that
//! expands it. The `#[no_mangle]` exports themselves are that macro's;
//! this module carries only what they delegate to.

mod downcall;
mod memory;

pub use downcall::serve;
pub use kobako_transport::abi::{pack_ptr_len, unpack_ptr_len, ABI_VERSION};
pub use memory::{alloc, answer};

// ---------------------------------------------------------------------------
// Host import declaration.
// ---------------------------------------------------------------------------
//
// The `wasm_import_module = "env"` attribute pins the import namespace.
// Signature: `(ptr: i32, len: i32) -> i64` per docs/wire/abi.md
// § ABI Signatures. We only declare the import on the wasm32 target —
// on the host target (where the rlib's unit tests run) there is no
// host to provide the symbol.
#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "env")]
extern "C" {
    /// Host-provided upcall. Guest writes a request at
    /// `[ptr, ptr + len)` and calls this; host returns a packed u64
    /// holding (ptr, len) of an answer the host allocated via
    /// `__kobako_alloc` inside the same call frame.
    /// Crate-internal — guests dispatch through `proxy`,
    /// never the raw import.
    pub(crate) fn __kobako_upcall(ptr: u32, len: u32) -> u64;
}
