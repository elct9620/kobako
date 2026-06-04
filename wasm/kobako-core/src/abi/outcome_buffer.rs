//! Per-invocation outcome buffer plus the host-driven I/O entry
//! points that frame it.
//!
//! The buffer is a static `Vec<u8>` written once per `__kobako_eval` /
//! `__kobako_run` invocation, then read by the host through
//! `__kobako_take_outcome` before the next invocation overwrites it.
//! The single-threaded wasm execution model guarantees the buffer is
//! never accessed concurrently inside a single wasm instance.
//!
//! `alloc` is the companion guest allocator behind `__kobako_alloc` —
//! the host calls that export from inside the `__kobako_dispatch`
//! host import. It delegates to wasi-libc `malloc` and lives here
//! because its lifetime is bounded by the same invocation that owns
//! the outcome buffer. The `#[no_mangle]` exports themselves are
//! emitted by `export_guest!` in the shell crate; these are the plain
//! functions they delegate to.

#[cfg(target_arch = "wasm32")]
use crate::codec::Encode;
#[cfg(target_arch = "wasm32")]
use crate::outcome::{Outcome, Panic};

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
    /// invocation entry body; the reader is `take_outcome`. Host
    /// serialisation guarantees the two never run concurrently, so the
    /// interior `&mut` taken here cannot alias the slice surfaced via
    /// `Self::as_slice`.
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
/// by `take_outcome`. Protected by the single-threaded wasm execution
/// model.
#[cfg(target_arch = "wasm32")]
static OUTCOME_BUFFER: OutcomeBuffer = OutcomeBuffer::new();

/// Write `bytes` into the outcome buffer, replacing whatever was left
/// from the previous invocation.
#[cfg(target_arch = "wasm32")]
pub fn write_outcome(bytes: Vec<u8>) {
    OUTCOME_BUFFER.write(bytes);
}

/// Encode `panic` as an Outcome envelope and stamp it into the
/// outcome buffer. If encoding itself fails, the buffer stays
/// empty — the host treats `len = 0` as a wire violation and follows
/// the TrapError path (docs/behavior.md Error Scenarios).
#[cfg(target_arch = "wasm32")]
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
/// The buffer is owned by the static `OUTCOME_BUFFER`; the host must
/// consume the bytes before the next invocation rebuilds the buffer.
pub fn take_outcome() -> u64 {
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
