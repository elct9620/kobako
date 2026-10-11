//! Guest memory the host writes into, and the answers it reads back.
//!
//! Every buffer comes from wasi-libc `malloc` and lives until the instance
//! is dropped at the end of the invocation, so nothing is freed one by one.

/// Guest allocator behind the `__kobako_alloc` export: a `size`-byte
/// buffer the host writes into, or 0 when `malloc` cannot reserve one,
/// which the host attributes to the guest rather than the engine.
pub fn alloc(size: u32) -> u32 {
    #[cfg(target_arch = "wasm32")]
    {
        extern "C" {
            fn malloc(size: usize) -> *mut u8;
        }
        // SAFETY: wasi-libc `malloc` is a standard C-ABI allocator with
        // a well-defined interface (size in / pointer out). NULL on
        // failure is the only contractually invalid pointer we surface
        // (folded into the 0 return below).
        let ptr = unsafe { malloc(size as usize) };
        if ptr.is_null() {
            return 0;
        }
        ptr as u32
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = size;
        0
    }
}

/// Copy an encoded answer into a buffer the host can read, returning its
/// packed `(ptr << 32) | len`, or 0 when no buffer could hold it — which
/// the host refuses as an unframed answer.
pub fn answer(bytes: &[u8]) -> u64 {
    let Ok(len) = u32::try_from(bytes.len()) else {
        return 0;
    };
    let ptr = alloc(len);
    if ptr == 0 || len == 0 {
        return 0;
    }
    // SAFETY: `alloc` returned a `len`-byte buffer in the current
    // instance's linear memory; copying `bytes.len()` bytes into it is
    // in-bounds.
    unsafe {
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr as *mut u8, bytes.len());
    }
    super::pack_ptr_len(ptr, len)
}
