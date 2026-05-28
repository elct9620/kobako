//! Typed `Array` newtype around an Array-tagged `Value`.
//!
//! `Array` is `#[repr(transparent)]` over `Value` (which is itself
//! `#[repr(transparent)]` over `mrb_value`). The two share their
//! in-memory layout — `Array` is exactly an `mrb_value` known to carry
//! an mruby `Array`. Construction is by explicit unchecked cast from
//! `Value`; element operations cluster on the resulting newtype.
//!
//! Mirrors magnus's `src/r_array.rs`: factories live on `Ruby` /
//! `Mrb`, per-array ops (`push`, `entry`) live here.

#[cfg(target_arch = "wasm32")]
use crate::{Mrb, Value};
#[cfg(target_arch = "wasm32")]
use mruby_sys as sys;

/// Typed handle on an mruby `Array`. `#[repr(transparent)]` over
/// `Value` so the C ABI is preserved.
///
/// Construct via `Mrb::ary_new` (fresh array) or
/// `Array::from_value_unchecked` (assert that a `Value` you
/// already hold is Array-tagged). Round-trip back to a generic
/// `Value` via `Array::as_value` for APIs that take any value.
#[cfg(target_arch = "wasm32")]
#[repr(transparent)]
#[derive(Copy, Clone)]
pub struct Array(Value);

#[cfg(target_arch = "wasm32")]
impl Array {
    /// Wrap a `Value` that the caller has already determined to be
    /// Array-tagged (e.g. via a `classname` check or because it came
    /// straight from `mrb_ary_new` / a host array decoder).
    ///
    /// # Safety
    ///
    /// `v` must be Array-tagged. Operating on a non-Array value
    /// through this newtype is undefined per mruby's macro contract
    /// (the underlying `mrb_ary_*` calls assume Array layout).
    #[inline]
    pub unsafe fn from_value_unchecked(v: Value) -> Self {
        Self(v)
    }

    /// Reify as a generic `Value` for APIs that accept any value.
    #[inline]
    pub fn as_value(self) -> Value {
        self.0
    }

    /// Borrow the inner `mrb_value` for raw FFI calls that have not
    /// yet migrated. Same conversion ladder as
    /// `Value::as_raw`.
    #[inline]
    pub fn as_raw(self) -> sys::mrb_value {
        self.0.as_raw()
    }

    /// `mrb_ary_push(mrb, self, val)` — append `val` to this array.
    #[inline]
    pub fn push(self, mrb: &Mrb, val: Value) {
        // SAFETY: `mrb` is alive; `self` is Array-tagged by the
        // `from_value_unchecked` contract; `val` originates from the
        // same VM by the single-VM contract.
        unsafe { sys::mrb_ary_push(mrb.as_ptr(), self.0.as_raw(), val.as_raw()) };
    }

    /// `mrb_ary_entry(self, idx)` — read the element at `idx`.
    /// Returns `mrb_nil_value()` when `idx` is out of range — mruby's
    /// own bounds-tolerant behaviour. The type guarantee from the
    /// constructor makes this safe for any in-range or out-of-range
    /// integer `idx`.
    #[inline]
    pub fn entry(self, idx: i32) -> Value {
        // SAFETY: `self` is Array-tagged by the `from_value_unchecked`
        // contract; `mrb_ary_entry` is bounds-tolerant.
        Value::from_raw(unsafe { sys::mrb_ary_entry(self.0.as_raw(), idx) })
    }
}
