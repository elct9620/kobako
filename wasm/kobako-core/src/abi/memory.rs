//! Guest memory the host writes into, and the answers it reads back.
//!
//! Every buffer comes from wasi-libc's allocator and is freed once read, as
//! the Canonical ABI has a list's receiver own it. A run makes Calls without
//! end, so a buffer kept to the end of the invocation would fill its memory.

#[cfg(target_arch = "wasm32")]
extern "C" {
    fn aligned_alloc(align: usize, size: usize) -> *mut u8;
    #[link_name = "free"]
    fn libc_free(ptr: *mut u8);
}

/// Give back the `size` bytes at `ptr`, taken at `align` through
/// `cabi_realloc`, once nothing reads them, as wit-bindgen's `cabi_dealloc`
/// takes. An empty list took no room, so nothing is freed for it.
///
/// # Safety
///
/// `[ptr, ptr + size)` came from `cabi_realloc` at `align`, and nothing
/// reads it after.
pub unsafe fn dealloc(ptr: u32, size: u32, align: u32) {
    #[cfg(target_arch = "wasm32")]
    if size != 0 {
        // wasi-libc's `free` takes whatever its `aligned_alloc` gave, at any
        // alignment.
        let _ = align;
        // SAFETY: the caller's promise above.
        unsafe { libc_free(ptr as *mut u8) };
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = (ptr, size, align);
}

/// The Canonical ABI's `cabi_realloc` behind the export of that name: room
/// for `new_len` bytes at `align`, holding what stood at `old_ptr` when the
/// host grows a list, whose old buffer is then freed. An empty list takes
/// no room and answers `align`, as wit-bindgen's runtime does; with no room
/// left it traps, so the host never writes at 0.
pub fn realloc(old_ptr: u32, old_len: u32, align: u32, new_len: u32) -> u32 {
    #[cfg(target_arch = "wasm32")]
    {
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
        if old_ptr != 0 && old_len != 0 {
            // SAFETY: the host handed back a buffer this guest allocated, of
            // `old_len` bytes, and the new one holds at least as many as
            // copied; the host reads the old one no more.
            unsafe {
                core::ptr::copy_nonoverlapping(
                    old_ptr as *const u8,
                    ptr,
                    old_len.min(new_len) as usize,
                );
                dealloc(old_ptr, old_len, align as u32);
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

/// The return area every downcall's Reply is named in. A downcall nested in
/// an upcall writes it too, but the host copies that Reply out before the
/// outer one is written.
#[cfg(target_arch = "wasm32")]
static RETURN_AREA: crate::instance_cell::InstanceCell<[u32; 2]> =
    crate::instance_cell::InstanceCell::new([0; 2]);

/// The Reply the return area names. The host copies a Reply out before the
/// next is written, so writing one frees the one before it.
#[cfg(target_arch = "wasm32")]
static RETURN_REPLY: crate::instance_cell::InstanceCell<Vec<u8>> =
    crate::instance_cell::InstanceCell::new(Vec::new());

/// Keep an encoded Reply in guest memory and return the return area that
/// names its `(ptr, len)`, for the downcall export to return.
pub fn return_reply(bytes: &[u8]) -> u32 {
    #[cfg(target_arch = "wasm32")]
    {
        let reply = RETURN_REPLY.get();
        let area = RETURN_AREA.get();
        // SAFETY: no borrow of either cell is live, and the host reads them
        // only after the export that wrote them returns.
        unsafe {
            *reply = bytes.to_vec();
            *area = [(*reply).as_ptr() as u32, (*reply).len() as u32];
        }
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
