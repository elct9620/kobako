//! RAII wrapper around mruby's `mrb_ccontext *`.
//!
//! Three guest entry points compile and evaluate Ruby source through
//! the same four-step lifecycle:
//!
//!   1. `mrb_ccontext_new(mrb)` — allocate the compile context.
//!   2. `mrb_ccontext_filename(mrb, cxt, name)` — stamp a filename so
//!      the produced IREP carries `debug_info` (required for
//!      `Exception#backtrace`, per
//!      `vendor/mruby/src/backtrace.c::pack_backtrace`).
//!   3. `mrb_load_nstring_cxt(mrb, ptr, len, cxt)` — compile + run.
//!   4. `mrb_ccontext_free(mrb, cxt)` — release the context.
//!
//! Before this module the four calls sat inline at every site
//! (`abi::boot::replay_snippets`, `abi::eval::eval_body`,
//! `abi::run::run_body`), each guarded by its own `unsafe { ... }`
//! block and a manual NULL check. The wrapper collapses that to one
//! `Ccontext::new(&mrb, c"...")` + `cxt.load_nstring(bytes)` pair;
//! `Drop` runs the free unconditionally.

use crate as sys;
use crate::Mrb;
use crate::Value;

/// Owned mruby compile context, tied to the lifetime of an [`Mrb`].
///
/// The lifetime parameter prevents the context from outliving the
/// `mrb_state` that produced it: when [`Drop`] runs we still need
/// `self.mrb.as_ptr()` to call `mrb_ccontext_free`, and the borrow
/// checker keeps `Mrb` alive long enough.
pub struct Ccontext<'mrb> {
    mrb: &'mrb Mrb,
    raw: *mut sys::mrb_ccontext,
}

impl<'mrb> Ccontext<'mrb> {
    /// Allocate a fresh compile context and stamp it with `filename`.
    /// Returns `None` when `mrb_ccontext_new` returns NULL — callers
    /// map that to a `Kobako::BootError` Panic.
    ///
    /// `mrb_ccontext_filename` interns the bytes, so the `&CStr`
    /// borrow only has to outlive this call.
    pub fn new(mrb: &'mrb Mrb, filename: &core::ffi::CStr) -> Option<Self> {
        // SAFETY: `mrb` is live by the borrow.
        let raw = unsafe { sys::mrb_ccontext_new(mrb.as_ptr()) };
        if raw.is_null() {
            return None;
        }
        // SAFETY: `mrb` is live; `raw` was just produced by the
        // matching `mrb_ccontext_new`; `filename.as_ptr()` is a
        // NUL-terminated `*const c_char` by `CStr`'s invariant.
        unsafe { sys::mrb_ccontext_filename(mrb.as_ptr(), raw, filename.as_ptr()) };
        Some(Self { mrb, raw })
    }

    /// Compile and evaluate `source` under this context. `source` is
    /// raw bytes (ptr + len), not NUL-terminated.
    pub fn load_nstring(&self, source: &[u8]) -> Value {
        // SAFETY: `self.mrb` is live by the borrow; `self.raw` was
        // produced by `mrb_ccontext_new` in `Self::new` and is owned
        // for the lifetime of `&self`; the source bytes outlive the
        // call because `mrb_load_nstring_cxt` does not retain a
        // reference past return.
        Value::from_raw(unsafe {
            sys::mrb_load_nstring_cxt(
                self.mrb.as_ptr(),
                source.as_ptr() as *const core::ffi::c_char,
                source.len(),
                self.raw,
            )
        })
    }
}

impl Drop for Ccontext<'_> {
    fn drop(&mut self) {
        // SAFETY: `self.mrb` is alive per the borrow; `self.raw` was
        // produced by `mrb_ccontext_new` and has not been freed yet
        // (`Self` is the sole owner).
        unsafe { sys::mrb_ccontext_free(self.mrb.as_ptr(), self.raw) };
    }
}
