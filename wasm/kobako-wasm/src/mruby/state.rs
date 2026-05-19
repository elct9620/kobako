//! RAII wrapper around mruby's `mrb_state *`.
//!
//! [`Mrb`] owns a freshly opened mruby VM. [`Mrb::open`] allocates a new
//! state via `mrb_open`; [`Drop`] releases it via `mrb_close`. Callers
//! that still reach for the raw FFI (during the staged migration) use
//! [`Mrb::as_ptr`] as an explicit escape hatch.
//!
//! `Mrb` is intentionally `!Send` and `!Sync` (inherited from
//! `NonNull<mrb_state>`): mruby's `mrb_state` is single-threaded and
//! must not cross thread boundaries.
//!
//! ## Why a newtype rather than passing `*mut mrb_state`
//!
//! Two problems with the raw pointer:
//!
//! 1. Every function that takes one must be `unsafe fn` even when it
//!    does nothing more than forward to FFI — "unsafe contagion" across
//!    every helper that touches the VM.
//! 2. Manual `mrb_close` calls scatter across every panic-outcome path
//!    in `__kobako_eval`. Forgetting one is a quiet memory leak the
//!    type system cannot catch.
//!
//! `Mrb` fixes both: the owning type makes "the VM is live" provable by
//! the borrow checker, and `Drop` makes `mrb_close` automatic.

#[cfg(target_arch = "wasm32")]
use crate::mruby::sys;
#[cfg(target_arch = "wasm32")]
use core::ptr::NonNull;

/// Owning handle to a live mruby VM. Closed automatically on drop.
///
/// On non-wasm32 targets the inner pointer field is absent because
/// [`Mrb::open`] always returns `Err` there; the type still compiles so
/// that `Result<Mrb, MrbOpenError>` is a uniform return type across
/// targets.
pub struct Mrb {
    #[cfg(target_arch = "wasm32")]
    state: NonNull<sys::mrb_state>,
}

/// Returned by [`Mrb::open`] when `mrb_open` returns NULL (allocation
/// failure inside mruby) or on the host target where `mrb_open` is not
/// linked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MrbOpenError;

impl std::fmt::Display for MrbOpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("mrb_open returned NULL")
    }
}

impl std::error::Error for MrbOpenError {}

impl Mrb {
    /// Open a fresh mruby state. Returns [`MrbOpenError`] when mruby's
    /// allocator cannot produce a state (or unconditionally on the host
    /// target — the mruby C API is not linked into the rlib).
    pub fn open() -> Result<Self, MrbOpenError> {
        #[cfg(target_arch = "wasm32")]
        {
            let raw = unsafe { sys::mrb_open() };
            NonNull::new(raw)
                .map(|state| Self { state })
                .ok_or(MrbOpenError)
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            Err(MrbOpenError)
        }
    }

    /// Raw `*mut mrb_state`. Use only at FFI boundaries that have not
    /// yet migrated to safe methods on `Mrb`. The returned pointer is
    /// valid for the lifetime of `&self`; callers must not call
    /// `mrb_close` on it (the `Mrb` Drop owns that). wasm32-only —
    /// host targets cannot construct an `Mrb`, so the raw-pointer
    /// escape hatch has no callers there.
    #[cfg(target_arch = "wasm32")]
    #[inline]
    pub fn as_ptr(&self) -> *mut sys::mrb_state {
        self.state.as_ptr()
    }

    /// Return the currently pending mruby exception, or
    /// `mrb_nil_value()` (`w == 0`) if none. Reads `mrb->exc` via the
    /// layout-safe C accessor [`sys::kobako_get_exc`]; does NOT clear
    /// the field — callers pair this with [`Mrb::clear_exc`] after
    /// they have captured class/message/backtrace.
    #[cfg(target_arch = "wasm32")]
    pub fn pending_exc(&self) -> sys::mrb_value {
        // SAFETY: `self.state` is alive by the `&self` borrow.
        unsafe { sys::kobako_get_exc(self.as_ptr()) }
    }

    /// Clear `mrb->exc`. Idempotent; safe to call when no exception
    /// is pending. Used by [`crate::abi::boot::take_pending_panic`]
    /// after the pending exception has been extracted, so subsequent
    /// mruby calls do not observe stale exception state.
    #[cfg(target_arch = "wasm32")]
    pub fn clear_exc(&self) {
        // SAFETY: `self.state` is alive by the `&self` borrow. The
        // return value (a `mrb_bool` snapshot of the prior `mrb->exc`
        // state) is intentionally discarded.
        let _ = unsafe { sys::mrb_check_error(self.as_ptr()) };
    }
}

#[cfg(target_arch = "wasm32")]
impl Drop for Mrb {
    fn drop(&mut self) {
        // SAFETY: `state` was produced by `mrb_open` in `Mrb::open` and
        // has not been closed elsewhere — `as_ptr` hands out borrows but
        // never takes ownership.
        unsafe { sys::mrb_close(self.state.as_ptr()) };
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for Mrb {
    fn drop(&mut self) {
        // Unreachable: `Mrb::open` always returns `Err` on host targets,
        // so no `Mrb` value can be constructed there. Required only so
        // the type satisfies `Drop` uniformly across targets.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_returns_error_on_host_target() {
        // Host target: `mrb_open` is not linked, so `open` must yield
        // `Err` without attempting an FFI call. This is the documented
        // host-side contract; wasm32 coverage runs through the E2E
        // journeys.
        assert_eq!(Mrb::open().err(), Some(MrbOpenError));
    }
}
