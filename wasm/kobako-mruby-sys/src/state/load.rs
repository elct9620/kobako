//! RITE / kobako bytecode loaders on [`Mrb`].
//!
//! Inherent methods that drop a compiled blob into the live mruby VM
//! and run its top-level Proc.

#[cfg(target_arch = "wasm32")]
use crate as sys;
#[cfg(target_arch = "wasm32")]
use crate::{Mrb, Value};

#[cfg(target_arch = "wasm32")]
impl Mrb {
    /// `mrb_load_irep_buf(mrb, buf, size)` — load and evaluate a
    /// precompiled RITE bytecode blob. On a malformed blob mruby
    /// sets `mrb->exc`; callers should inspect via
    /// [`Mrb::pending_exc`] before continuing.
    #[inline]
    pub fn load_irep_buf(&self, bytes: &[u8]) -> Value {
        // SAFETY: `self` is alive; `bytes` is borrowed for the
        // synchronous call.
        Value::from_raw(unsafe {
            sys::mrb_load_irep_buf(
                self.as_ptr(),
                bytes.as_ptr() as *const core::ffi::c_void,
                bytes.len(),
            )
        })
    }

    /// `kobako_load_bytecode(mrb, buf, size)` — load + validate +
    /// execute a `#preload(binary:)` snippet. Returns 0 on success
    /// and non-zero on structural failure (E-37 / E-38). Top-level
    /// exceptions from a successful load are left in `mrb->exc` for
    /// downstream extraction.
    #[inline]
    pub fn load_bytecode(&self, bytes: &[u8]) -> core::ffi::c_int {
        // SAFETY: as `load_irep_buf`.
        unsafe {
            sys::kobako_load_bytecode(
                self.as_ptr(),
                bytes.as_ptr() as *const core::ffi::c_void,
                bytes.len(),
            )
        }
    }
}
