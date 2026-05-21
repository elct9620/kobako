//! String / Array / Hash factories on [`Mrb`].
//!
//! Magnus's `Ruby` token holds factories like `ary_new` / `hash_new`
//! / `str_new` as **inherent methods**, not on traits. We follow that
//! pattern: this file adds inherent factory methods to [`Mrb`] via an
//! `impl Mrb` block, mirroring magnus's `src/api.rs` style.
//!
//! Collection factories (`ary_new`, `hash_new`) return typed newtypes
//! [`Array`] / [`Hash`] so per-collection operations (`push`, `set`,
//! `get`, `keys`) cluster on the value type rather than on `Mrb` —
//! the magnus shape (see `src/r_array.rs` / `src/r_hash.rs`).

#[cfg(target_arch = "wasm32")]
use crate as sys;
#[cfg(target_arch = "wasm32")]
use crate::{Mrb, Array, Hash, Value};

#[cfg(target_arch = "wasm32")]
impl Mrb {
    /// `mrb_str_new(mrb, p, len)` — construct an mruby `String` from
    /// `bytes`. The buffer is copied into the mruby heap; the slice
    /// only has to live for the duration of the call.
    ///
    /// `bytes.len()` saturates to [`i32::MAX`] (mruby's `mrb_int` on
    /// wasm32 is signed 32-bit). Real callers never reach that — the
    /// host-side String size cap (8 MiB) sits well below.
    #[inline]
    pub fn str_new(&self, bytes: &[u8]) -> Value {
        let len = bytes.len().min(i32::MAX as usize) as i32;
        // SAFETY: `self` is alive by the `&self` borrow; `bytes`
        // outlives the synchronous call.
        Value::from_raw(unsafe {
            sys::mrb_str_new(
                self.as_ptr(),
                bytes.as_ptr() as *const core::ffi::c_char,
                len,
            )
        })
    }

    /// `mrb_str_new_cstr(mrb, s)` — construct an mruby `String` from
    /// a NUL-terminated C string. The `&CStr` borrow guarantees the
    /// terminator.
    #[inline]
    pub fn str_new_cstr(&self, s: &core::ffi::CStr) -> Value {
        // SAFETY: `self` is alive; `s.as_ptr()` is NUL-terminated by
        // the `&CStr` contract.
        Value::from_raw(unsafe { sys::mrb_str_new_cstr(self.as_ptr(), s.as_ptr()) })
    }

    /// `mrb_ary_new(mrb)` — construct a fresh empty mruby `Array` as
    /// a typed [`Array`]. Element operations (`push`, `entry`) live
    /// on the returned newtype.
    #[inline]
    pub fn ary_new(&self) -> Array {
        // SAFETY: `self` is alive; `mrb_ary_new` always returns an
        // Array-tagged value, so the unchecked wrap is sound.
        unsafe { Array::from_value_unchecked(Value::from_raw(sys::mrb_ary_new(self.as_ptr()))) }
    }

    /// `mrb_hash_new(mrb)` — construct a fresh empty mruby `Hash` as
    /// a typed [`Hash`]. Element operations (`set`, `get`, `keys`)
    /// live on the returned newtype.
    #[inline]
    pub fn hash_new(&self) -> Hash {
        // SAFETY: `self` is alive; `mrb_hash_new` always returns a
        // Hash-tagged value, so the unchecked wrap is sound.
        unsafe { Hash::from_value_unchecked(Value::from_raw(sys::mrb_hash_new(self.as_ptr()))) }
    }
}
