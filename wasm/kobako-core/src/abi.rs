//! Guest ABI machinery: the host imports (`upcall`, `preamble`), the guest
//! memory the host writes into (`realloc`), the downcall a guest answers
//! (`handle`, its Reply handed back through `return_reply`), and the one run of the static
//! constructors (`run_ctors_once`).
//!
//! The names the ABI fixes are `kobako_transport::abi`'s. The exports
//! themselves are `crate::export_guest!`'s; this module carries only what
//! they delegate to.

mod downcall;
mod memory;

pub use downcall::handle;
pub use memory::{realloc, return_reply, run_ctors_once};

/// Declare the host imports under the host interface's literal name, the
/// only form `wasm_import_module` accepts.
#[cfg(target_arch = "wasm32")]
macro_rules! host_imports {
    ($module:literal) => {
        #[link(wasm_import_module = $module)]
        extern "C" {
            /// The guest writes a Call at `[ptr, ptr + len)`; the host writes
            /// the Reply into room it took from `cabi_realloc` and names it in
            /// the two u32 at `ret`. Crate-internal: guests dispatch through
            /// `proxy`, never the raw import.
            pub(crate) fn upcall(ptr: u32, len: u32, ret: u32);

            /// The host writes this Invocation's Preamble into room it took
            /// from `cabi_realloc` and names it in the two u32 at `ret`.
            pub(crate) fn preamble(ret: u32);
        }
    };
}

#[cfg(target_arch = "wasm32")]
kobako_transport::abi_name!(host => host_imports);

/// The `(ptr, len)` a host names in a return area, as the bytes it holds.
/// An empty list names nothing to read.
///
/// # Safety
///
/// The host wrote `area` and promises `[ptr, ptr + len)` lies in this
/// instance's memory, which lives until the instance is dropped.
#[cfg(target_arch = "wasm32")]
pub(crate) unsafe fn take_list(area: [u32; 2]) -> Vec<u8> {
    let [ptr, len] = area;
    if len == 0 {
        return Vec::new();
    }
    // SAFETY: the caller's promise above.
    unsafe { core::slice::from_raw_parts(ptr as *const u8, len as usize) }.to_vec()
}
