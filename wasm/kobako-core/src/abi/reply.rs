//! The Yield Reply a `__kobako_yield_to_block` invocation answers with.

use kobako_transport::envelope::YieldReply;

/// Frame `reply` into a buffer the host can read, answering its packed
/// `(ptr << 32) | len`, or 0 when no buffer could hold it — which the
/// host refuses as an unframed answer.
///
/// The buffer comes from `alloc`, so it lives until the instance is
/// dropped at the end of the invocation.
pub fn write_reply(reply: YieldReply) -> u64 {
    let bytes = reply.encode();
    let Ok(len) = u32::try_from(bytes.len()) else {
        return 0;
    };
    let ptr = super::alloc(len);
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
