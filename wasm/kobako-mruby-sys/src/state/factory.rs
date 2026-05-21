//! String / Array / Hash factories on [`Mrb`].
//!
//! Magnus's `Ruby` token holds factories like `ary_new` /
//! `hash_new` / `str_new` as **inherent methods**, not on traits.
//! We follow that pattern: this file adds inherent factory methods
//! to [`Mrb`] via an `impl Mrb` block, mirroring magnus's
//! `src/api.rs` style.

#[cfg(target_arch = "wasm32")]
use crate as sys;
#[cfg(target_arch = "wasm32")]
use crate::{Mrb, Value};

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

    /// `mrb_ary_new(mrb)` — construct a fresh empty mruby `Array`.
    ///
    /// **Transitional API:** returns a generic [`Value`] for now;
    /// a follow-up commit replaces this with a typed `RArray`
    /// return and moves element ops (`ary_push`, `ary_entry`) onto
    /// that newtype.
    #[inline]
    pub fn ary_new(&self) -> Value {
        // SAFETY: `self` is alive.
        Value::from_raw(unsafe { sys::mrb_ary_new(self.as_ptr()) })
    }

    /// `mrb_ary_push(mrb, ary, val)` — append `val` to `ary`. `ary`
    /// must be an Array-tagged [`Value`] produced by the same VM.
    #[inline]
    pub fn ary_push(&self, ary: Value, val: Value) {
        // SAFETY: `self` is alive; both values originate from the
        // same VM by the single-VM contract.
        unsafe { sys::mrb_ary_push(self.as_ptr(), ary.as_raw(), val.as_raw()) };
    }

    /// `mrb_hash_new(mrb)` — construct a fresh empty mruby `Hash`.
    ///
    /// **Transitional API:** see [`Mrb::ary_new`].
    #[inline]
    pub fn hash_new(&self) -> Value {
        // SAFETY: `self` is alive.
        Value::from_raw(unsafe { sys::mrb_hash_new(self.as_ptr()) })
    }

    /// `mrb_hash_set(mrb, hash, key, val)` — assign `key => val` in
    /// `hash`.
    #[inline]
    pub fn hash_set(&self, hash: Value, key: Value, val: Value) {
        // SAFETY: as `ary_push`.
        unsafe { sys::mrb_hash_set(self.as_ptr(), hash.as_raw(), key.as_raw(), val.as_raw()) };
    }

    /// `mrb_hash_get(mrb, hash, key)` — return the value for `key`,
    /// or `nil` when absent.
    #[inline]
    pub fn hash_get(&self, hash: Value, key: Value) -> Value {
        // SAFETY: as `ary_push`.
        Value::from_raw(unsafe { sys::mrb_hash_get(self.as_ptr(), hash.as_raw(), key.as_raw()) })
    }

    /// `mrb_hash_keys(mrb, hash)` — return the Array of keys in
    /// `hash`.
    #[inline]
    pub fn hash_keys(&self, hash: Value) -> Value {
        // SAFETY: as `ary_push`.
        Value::from_raw(unsafe { sys::mrb_hash_keys(self.as_ptr(), hash.as_raw()) })
    }
}
