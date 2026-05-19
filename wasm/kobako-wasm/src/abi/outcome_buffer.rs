//! Per-invocation outcome buffer plus the host-driven I/O exports that
//! frame it.
//!
//! The buffer is a static `Vec<u8>` written once per `__kobako_eval` /
//! `__kobako_run` invocation, then read by the host through
//! [`__kobako_take_outcome`] before the next invocation overwrites it.
//! The single-threaded wasm execution model guarantees the buffer is
//! never accessed concurrently inside a single wasm instance.
//!
//! [`__kobako_alloc`] is the companion guest allocator the host calls
//! from inside the `__kobako_dispatch` host import — it delegates to
//! wasi-libc `malloc` and lives here because its lifetime is bounded
//! by the same invocation that owns the outcome buffer.

#[cfg(target_arch = "wasm32")]
use crate::outcome::{encode_outcome, Outcome, Panic};

#[cfg(target_arch = "wasm32")]
use super::pack_u64;

/// Static outcome buffer — written once per invocation, consumed once
/// by `__kobako_take_outcome`. Protected by the single-threaded wasm
/// execution model.
#[cfg(target_arch = "wasm32")]
static mut OUTCOME_BUFFER: Vec<u8> = Vec::new();

/// Write `bytes` into [`OUTCOME_BUFFER`], replacing whatever was left
/// from the previous invocation.
///
/// # Safety
///
/// The outcome buffer is touched only inside `__kobako_eval` /
/// `__kobako_run` (writes) and `__kobako_take_outcome` (reads). Wasm
/// runs single-threaded inside one instance, and the host serializes
/// the invocation around its `take_outcome!` read, so the `&mut`
/// asserted by this write cannot alias the buffer read by the
/// take-outcome path.
#[cfg(target_arch = "wasm32")]
pub(super) fn write_outcome(bytes: Vec<u8>) {
    unsafe {
        OUTCOME_BUFFER = bytes;
    }
}

/// Encode `panic` as an Outcome envelope and stamp it into
/// [`OUTCOME_BUFFER`]. If encoding itself fails, the buffer stays
/// empty — the host treats `len = 0` as a wire violation and follows
/// the TrapError path (docs/behavior.md Error Scenarios).
#[cfg(target_arch = "wasm32")]
pub(super) fn write_panic(panic: Panic) {
    if let Ok(bytes) = encode_outcome(&Outcome::Panic(panic)) {
        write_outcome(bytes);
    }
}

/// Guest allocator — hands out a `size`-byte buffer in wasm linear
/// memory and returns its ptr (u32). Returns 0 on allocation failure
/// (host treats 0 as a trap signal). Signature: `(size: i32) -> i32`.
///
/// Delegates to wasi-libc's `malloc`. The allocated buffer is
/// intentionally not freed — its lifetime is bounded by the wasm
/// instance lifetime (one Sandbox invocation). The host writes the RPC
/// response into this buffer inside the `__kobako_dispatch` callback,
/// then consumes the response synchronously before the RPC call
/// returns, so the buffer does not need to outlive the call frame.
/// Instance drop frees all linear memory.
#[no_mangle]
pub extern "C" fn __kobako_alloc(size: u32) -> u32 {
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

/// Outcome reader — host calls this after `__kobako_eval` /
/// `__kobako_run` returns to fetch the [`OUTCOME_BUFFER`] bytes.
/// Returns packed u64 `(ptr << 32) | len`. `len == 0` is a wire
/// violation (docs/wire-codec.md § ABI Signatures). Signature:
/// `() -> i64`.
///
/// The buffer is owned by the static `OUTCOME_BUFFER`; the host must
/// consume the bytes before the next invocation rebuilds the buffer.
#[no_mangle]
pub extern "C" fn __kobako_take_outcome() -> u64 {
    #[cfg(target_arch = "wasm32")]
    {
        // SAFETY: see [`write_outcome`] — single-threaded wasm
        // execution serialises reads against writes.
        let bytes = &raw const OUTCOME_BUFFER;
        let bytes = unsafe { &*bytes };
        if bytes.is_empty() {
            return 0;
        }
        let ptr = bytes.as_ptr() as u32;
        let len = bytes.len() as u32;
        pack_u64(ptr, len)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        0
    }
}
