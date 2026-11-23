//! `random_get`, answered a page at a time into the guest's own buffer, as
//! Linux's `get_random_bytes_user` copies a page at a time: the buffer is
//! checked before any entropy is drawn, the host holds one page, and a run
//! past its deadline or interrupted stops between pages, as `getrandom`
//! looks for a pending signal at each page boundary.

use wasmtime_wasi::p2::bindings::random::random::Host as _;
use wiggle::GuestPtr;

use super::errno::Answer;
use super::memory::GuestMemory;
use super::types;
use crate::trap;
use crate::wasi::Wasi;

/// What one step draws and copies, Linux's `PAGE_SIZE`.
const PAGE: usize = 4096;

/// Fill the `len` bytes at `buf` with the entropy `wasi`'s profile reads.
pub(super) fn get(
    memory: &mut GuestMemory<'_, '_>,
    wasi: &mut Wasi,
    buf: GuestPtr<u8>,
    len: u32,
) -> Answer {
    let bytes = memory.bytes_mut(buf.as_array(len))?;
    let random = wasmtime_wasi::WasiView::ctx(wasi.ctx_mut()).ctx.random();
    for (index, page) in bytes.chunks_mut(PAGE).enumerate() {
        if index > 0 {
            if let Some(fatal) = trap::fatal_signal_trap() {
                return Err(types::Error::trap(fatal));
            }
        }
        let drawn = random
            .get_random_bytes(page.len() as u64)
            .map_err(types::Error::trap)?;
        page.copy_from_slice(&drawn);
    }
    Ok(())
}
