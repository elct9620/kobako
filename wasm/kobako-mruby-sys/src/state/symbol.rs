//! Symbol intern + lookup on [`Mrb`].
//!
//! Inherent methods that turn a name (NUL-terminated `&CStr` or
//! arbitrary bytes via an `mrb_value` String) into an `mrb_sym`, or
//! read the C-string name back from a symbol id.

#[cfg(target_arch = "wasm32")]
use crate as sys;
#[cfg(target_arch = "wasm32")]
use crate::{Mrb, Value};

#[cfg(target_arch = "wasm32")]
impl Mrb {
    /// `mrb_intern_cstr(mrb, s)` — intern a NUL-terminated C string
    /// as a Symbol id.
    #[inline]
    pub fn intern_cstr(&self, s: &core::ffi::CStr) -> sys::mrb_sym {
        // SAFETY: `self` is alive; `s.as_ptr()` is NUL-terminated by
        // the `&CStr` contract.
        unsafe { sys::mrb_intern_cstr(self.as_ptr(), s.as_ptr()) }
    }

    /// `mrb_intern_str(mrb, str)` — intern the bytes of an mruby
    /// String value as a Symbol. Use this when the name arrives as
    /// arbitrary bytes that may not be NUL-safe; otherwise prefer
    /// [`Mrb::intern_cstr`].
    #[inline]
    pub fn intern_str(&self, s: Value) -> sys::mrb_sym {
        // SAFETY: `self` is alive; `s` originates from the same VM.
        unsafe { sys::mrb_intern_str(self.as_ptr(), s.as_raw()) }
    }

    /// `mrb_sym_name(mrb, sym)` — return the C string name of `sym`,
    /// or `None` if mruby yields a NULL pointer (e.g. uninterned id).
    /// The returned slice points into mruby's interned string storage
    /// and lives for the duration of the VM.
    #[inline]
    pub fn sym_name(&self, sym: sys::mrb_sym) -> Option<&'static str> {
        // SAFETY: `self` is alive.
        let ptr = unsafe { sys::mrb_sym_name(self.as_ptr(), sym) };
        if ptr.is_null() {
            return None;
        }
        // SAFETY: mruby's interned symbol storage lives for the
        // duration of the VM; treating the slice as `'static` is
        // sound for that lifetime, which the caller upholds via the
        // owning `Mrb`.
        Some(
            unsafe { core::ffi::CStr::from_ptr(ptr) }
                .to_str()
                .unwrap_or(""),
        )
    }
}
