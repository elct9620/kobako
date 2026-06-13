//! Per-invocation outcome buffer plus the host-driven I/O entry
//! points that frame it.
//!
//! The buffer is a static `Vec<u8>` written once per `__kobako_eval` /
//! `__kobako_run` invocation, then read by the host through
//! `__kobako_take_outcome` before the next invocation overwrites it.
//!
//! `alloc` is the companion guest allocator behind `__kobako_alloc` —
//! the host calls that export from inside the `__kobako_dispatch`
//! host import. It delegates to wasi-libc `malloc` and lives here
//! because its lifetime is bounded by the same invocation that owns
//! the outcome buffer. The `#[no_mangle]` exports themselves are
//! emitted by `export_guest!` in the shell crate; these are the plain
//! functions they delegate to.

use crate::codec::Encode;
use crate::outcome::{Outcome, Panic};

use std::sync::Mutex;

/// Static outcome buffer — written once per invocation, consumed once
/// by `take_outcome`. The `Mutex` keeps the static a sound safe API on
/// every target; on wasm32 (single-threaded, the production target)
/// the lock is uncontended by construction.
static OUTCOME_BUFFER: Mutex<Vec<u8>> = Mutex::new(Vec::new());

/// Lock the buffer, absorbing poisoning: the guest builds with
/// `panic = "abort"` so a poisoned lock is unobservable there, and on
/// host targets the stored bytes stay well-formed regardless (writes
/// replace the whole `Vec`).
fn lock_buffer() -> std::sync::MutexGuard<'static, Vec<u8>> {
    OUTCOME_BUFFER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Write `bytes` into the outcome buffer, replacing whatever was left
/// from the previous invocation.
pub fn write_outcome(bytes: Vec<u8>) {
    *lock_buffer() = bytes;
}

/// Encode `panic` as an Outcome envelope and stamp it into the
/// outcome buffer. If encoding itself fails, the buffer stays
/// empty — the host treats `len = 0` as a wire violation and follows
/// the TrapError path.
pub fn write_panic(panic: Panic) {
    if let Ok(bytes) = Outcome::Panic(panic).encode() {
        write_outcome(bytes);
    }
}

/// Guest allocator — hands out a `size`-byte buffer in wasm linear
/// memory and returns its ptr (u32). Returns 0 on allocation failure
/// (host treats 0 as a trap signal). Behind the `__kobako_alloc`
/// export: signature `(size: i32) -> i32`.
///
/// Delegates to wasi-libc's `malloc`. The allocated buffer is
/// intentionally not freed — its lifetime is bounded by the wasm
/// instance lifetime (one Sandbox invocation). The host writes the
/// transport response into this buffer inside the `__kobako_dispatch`
/// callback, then consumes the response synchronously before the
/// transport call returns, so the buffer does not need to outlive the
/// call frame.
/// Instance drop frees all linear memory.
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

/// Outcome reader — the host calls the `__kobako_take_outcome` export
/// after `__kobako_eval` / `__kobako_run` returns to fetch the
/// outcome-buffer bytes. Returns packed u64 `(ptr << 32) | len`.
/// `len == 0` is a wire violation (docs/wire-codec.md § ABI
/// Signatures).
///
/// The returned ptr aliases the buffer the static owns; the host must
/// consume the bytes before the next invocation rebuilds the buffer.
/// The 32-bit ptr packing only means anything in wasm linear memory,
/// so the host target returns the 0 stub.
pub fn take_outcome() -> u64 {
    #[cfg(target_arch = "wasm32")]
    {
        let bytes = lock_buffer();
        if bytes.is_empty() {
            return 0;
        }
        let ptr = bytes.as_ptr() as u32;
        let len = bytes.len() as u32;
        super::pack_u64(ptr, len)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        0
    }
}
