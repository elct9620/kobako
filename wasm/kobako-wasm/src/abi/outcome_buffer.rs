//! Per-invocation outcome buffer plus the host-driven I/O exports that
//! frame it.
//!
//! The buffer is a static `Vec<u8>` written once per `__kobako_eval` /
//! `__kobako_run` invocation, then read by the host through
//! `__kobako_take_outcome` before the next invocation overwrites it.
//! The single-threaded wasm execution model guarantees the buffer is
//! never accessed concurrently inside a single wasm instance.
//!
//! `__kobako_alloc` is the companion guest allocator the host calls
//! from inside the `__kobako_dispatch` host import — it delegates to
//! wasi-libc `malloc` and lives here because its lifetime is bounded
//! by the same invocation that owns the outcome buffer.

#[cfg(target_arch = "wasm32")]
use kobako_core::codec::Encode;
#[cfg(target_arch = "wasm32")]
use kobako_core::outcome::{Outcome, Panic};

#[cfg(target_arch = "wasm32")]
use super::pack_u64;

#[cfg(target_arch = "wasm32")]
use core::cell::UnsafeCell;

/// Single-threaded interior-mutability wrapper for the per-invocation
/// outcome buffer. The `static mut Vec<u8>` shape is wrong twice over
/// (the Rust 2024 lints push toward `&raw` reborrows; the simpler
/// model is "this is a cell that wasm32 can mutate from a `&self`
/// view") — wrap once, document the single-threaded contract once,
/// stop sprinkling `unsafe` across the entry points.
#[cfg(target_arch = "wasm32")]
struct OutcomeBuffer(UnsafeCell<Vec<u8>>);

#[cfg(target_arch = "wasm32")]
impl OutcomeBuffer {
    const fn new() -> Self {
        Self(UnsafeCell::new(Vec::new()))
    }

    /// Replace the stored bytes. The writer is the wasm32-only
    /// `__kobako_eval` / `__kobako_run` reactor body; the reader is
    /// `__kobako_take_outcome`. Host serialisation guarantees the two
    /// never run concurrently, so the interior `&mut` taken here
    /// cannot alias the slice surfaced via `Self::as_slice`.
    fn write(&self, bytes: Vec<u8>) {
        // SAFETY: see type doc — single-threaded wasm execution + host
        // serialisation around `__kobako_take_outcome` guarantee no
        // aliasing.
        unsafe { *self.0.get() = bytes };
    }

    /// Borrow the stored bytes. Pointer arithmetic on the result is
    /// the host's contract: the returned slice lives until the next
    /// `Self::write` in the same wasm instance.
    fn as_slice(&self) -> &[u8] {
        // SAFETY: see type doc.
        unsafe { &*self.0.get() }
    }
}

// SAFETY: wasm32 is single-threaded; the buffer is never observed
// from more than one logical owner at a time inside a wasm instance.
// `static` requires `Sync` regardless of whether anyone could
// actually contend for it on this target.
#[cfg(target_arch = "wasm32")]
unsafe impl Sync for OutcomeBuffer {}

/// Static outcome buffer — written once per invocation, consumed once
/// by `__kobako_take_outcome`. Protected by the single-threaded wasm
/// execution model.
#[cfg(target_arch = "wasm32")]
static OUTCOME_BUFFER: OutcomeBuffer = OutcomeBuffer::new();

/// Write `bytes` into `OUTCOME_BUFFER`, replacing whatever was left
/// from the previous invocation.
#[cfg(target_arch = "wasm32")]
pub(super) fn write_outcome(bytes: Vec<u8>) {
    OUTCOME_BUFFER.write(bytes);
}

/// Encode `panic` as an Outcome envelope and stamp it into
/// `OUTCOME_BUFFER`. If encoding itself fails, the buffer stays
/// empty — the host treats `len = 0` as a wire violation and follows
/// the TrapError path (docs/behavior.md Error Scenarios).
#[cfg(target_arch = "wasm32")]
pub(super) fn write_panic(panic: Panic) {
    if let Ok(bytes) = Outcome::Panic(panic).encode() {
        write_outcome(bytes);
    }
}

/// Guest allocator — hands out a `size`-byte buffer in wasm linear
/// memory and returns its ptr (u32). Returns 0 on allocation failure
/// (host treats 0 as a trap signal). Signature: `(size: i32) -> i32`.
///
/// Delegates to wasi-libc's `malloc`. The allocated buffer is
/// intentionally not freed — its lifetime is bounded by the wasm
/// instance lifetime (one Sandbox invocation). The host writes the
/// transport response into this buffer inside the `__kobako_dispatch`
/// callback, then consumes the response synchronously before the
/// transport call returns, so the buffer does not need to outlive the
/// call frame.
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
/// `__kobako_run` returns to fetch the `OUTCOME_BUFFER` bytes.
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
        let bytes = OUTCOME_BUFFER.as_slice();
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
