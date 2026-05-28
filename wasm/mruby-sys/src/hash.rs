//! Typed `Hash` newtype around a Hash-tagged `Value`.
//!
//! `Hash` is `#[repr(transparent)]` over `Value` (which is itself
//! `#[repr(transparent)]` over `mrb_value`). The two share their
//! in-memory layout — `Hash` is exactly an `mrb_value` known to carry
//! an mruby `Hash`. Construction is by explicit unchecked cast from
//! `Value`; element operations cluster on the resulting newtype.
//!
//! Mirrors magnus's `src/r_hash.rs`: factories live on `Ruby` /
//! `Mrb`, per-hash ops (`set`, `get`, `keys`) live here.

#[cfg(target_arch = "wasm32")]
use crate as sys;
#[cfg(target_arch = "wasm32")]
use crate::{Array, Mrb, Value};

/// Typed handle on an mruby `Hash`. `#[repr(transparent)]` over
/// `Value` so the C ABI is preserved.
///
/// Construct via `Mrb::hash_new` (fresh hash) or
/// `Hash::from_value_unchecked` (assert that a `Value` you
/// already hold is Hash-tagged). Round-trip back to a generic
/// `Value` via `Hash::as_value` for APIs that take any value.
#[cfg(target_arch = "wasm32")]
#[repr(transparent)]
#[derive(Copy, Clone)]
pub struct Hash(Value);

#[cfg(target_arch = "wasm32")]
impl Hash {
    /// Wrap a `Value` that the caller has already determined to be
    /// Hash-tagged (e.g. via a `classname` check or because it came
    /// straight from `mrb_hash_new` / a host hash decoder).
    ///
    /// # Safety
    ///
    /// `v` must be Hash-tagged. Operating on a non-Hash value
    /// through this newtype is undefined per mruby's macro contract.
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
    /// yet migrated.
    #[inline]
    pub fn as_raw(self) -> sys::mrb_value {
        self.0.as_raw()
    }

    /// `mrb_hash_set(mrb, self, key, val)` — assign `key => val`.
    #[inline]
    pub fn set(self, mrb: &Mrb, key: Value, val: Value) {
        // SAFETY: `mrb` is alive; `self` is Hash-tagged by the
        // `from_value_unchecked` contract; `key` and `val` originate
        // from the same VM.
        unsafe { sys::mrb_hash_set(mrb.as_ptr(), self.0.as_raw(), key.as_raw(), val.as_raw()) };
    }

    /// `mrb_hash_get(mrb, self, key)` — return the value for `key`,
    /// or `nil` when absent.
    #[inline]
    pub fn get(self, mrb: &Mrb, key: Value) -> Value {
        // SAFETY: as `set`.
        Value::from_raw(unsafe { sys::mrb_hash_get(mrb.as_ptr(), self.0.as_raw(), key.as_raw()) })
    }

    /// `mrb_hash_keys(mrb, self)` — return the Array of keys as a
    /// typed `Array`.
    #[inline]
    pub fn keys(self, mrb: &Mrb) -> Array {
        // SAFETY: as `set`; `mrb_hash_keys` always returns an
        // Array-tagged value, so the unchecked wrap is sound.
        unsafe {
            Array::from_value_unchecked(Value::from_raw(sys::mrb_hash_keys(
                mrb.as_ptr(),
                self.0.as_raw(),
            )))
        }
    }
}
