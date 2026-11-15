//! Guest memory the host writes into, and the answers it reads back.
//!
//! Every buffer comes from wasi-libc's allocator and lives until the
//! instance is dropped at the end of the invocation, so nothing is freed one
//! by one.

/// The Canonical ABI's `cabi_realloc` behind the export of that name: room
/// for `new_len` bytes at `align`, holding what stood at `old_ptr` when the
/// host grows a list. An empty list takes no room and answers `align`, as
/// wit-bindgen's runtime does, since the bindings never free one; with no
/// room left it traps, so the host never writes at 0. The old buffer stays.
pub fn realloc(old_ptr: u32, old_len: u32, align: u32, new_len: u32) -> u32 {
    #[cfg(target_arch = "wasm32")]
    {
        extern "C" {
            fn aligned_alloc(align: usize, size: usize) -> *mut u8;
        }
        if old_len == 0 && new_len == 0 {
            return align;
        }
        let align = (align as usize).max(1);
        // `aligned_alloc` takes a size that is a multiple of its alignment.
        let size = (new_len as usize).div_ceil(align) * align;
        // SAFETY: wasi-libc `aligned_alloc` takes a power-of-two alignment,
        // which the Canonical ABI always passes, and a size it divides.
        let ptr = unsafe { aligned_alloc(align, size) };
        if ptr.is_null() {
            core::arch::wasm32::unreachable();
        }
        if old_ptr != 0 {
            // SAFETY: the host handed back a buffer this guest allocated, of
            // `old_len` bytes, and the new one holds at least as many as copied.
            unsafe {
                core::ptr::copy_nonoverlapping(
                    old_ptr as *const u8,
                    ptr,
                    old_len.min(new_len) as usize,
                );
            }
        }
        ptr as u32
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = (old_ptr, old_len, align, new_len);
        0
    }
}

/// The return area every downcall answer is named in. A downcall nested in
/// an upcall writes it too, but the host copies that answer out before the
/// outer one is written.
#[cfg(target_arch = "wasm32")]
static RETURN_AREA: crate::instance_cell::InstanceCell<[u32; 2]> =
    crate::instance_cell::InstanceCell::new([0; 2]);

/// Keep an encoded answer in guest memory and answer the return area that
/// names its `(ptr, len)`, for the downcall export to return.
pub fn answer(bytes: &[u8]) -> u32 {
    #[cfg(target_arch = "wasm32")]
    {
        let kept: &'static [u8] = Vec::leak(bytes.to_vec());
        let area = RETURN_AREA.get();
        // SAFETY: no borrow of the area is live, and the host reads it only
        // after the export that wrote it returns.
        unsafe { *area = [kept.as_ptr() as u32, kept.len() as u32] };
        area as u32
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = bytes;
        0
    }
}

/// Run the static constructors the first time an export other than
/// `cabi_realloc` is entered, as wit-bindgen's runtime does: the host may
/// take room before any other export runs. Calling `__wasm_call_ctors`
/// here also keeps wasm-ld from wrapping each export to run them, and
/// their destructors, on every call.
pub fn run_ctors_once() {
    #[cfg(target_arch = "wasm32")]
    {
        static RAN: crate::instance_cell::InstanceCell<bool> =
            crate::instance_cell::InstanceCell::new(false);
        extern "C" {
            fn __wasm_call_ctors();
        }
        // SAFETY: no borrow of the flag is live, and wasm-ld defines
        // `__wasm_call_ctors` for every module it links.
        unsafe {
            if !*RAN.get() {
                __wasm_call_ctors();
                *RAN.get() = true;
            }
        }
    }
}
