//! `mrb_get_args` shape-specific wrappers on [`Mrb`].
//!
//! Inherent methods mapping the four argument-unpack patterns kobako
//! bridges actually use. Each method maps one mruby format string to
//! a typed Rust return, hiding the variadic FFI plumbing:
//!
//!   - [`Mrb::get_args_o`]       — `"o"`  → single positional
//!   - [`Mrb::get_args_rest`]    — `"*"`  → rest array
//!   - [`Mrb::get_args_n_rest`]  — `"n*"` → symbol + rest array
//!   - [`Mrb::get_args_io`]      — `"io"` → integer + object
//!
//! The rest-form variants borrow the call frame's argv buffer; the
//! lifetime is tied to `&self`, which the bridge body holds for the
//! duration of the C call. mruby may set the rest pointer to NULL
//! when the rest count is zero — the helpers fold that into an empty
//! `&[Value]` so callers do not have to gate on NULL.

#[cfg(target_arch = "wasm32")]
use crate as sys;
#[cfg(target_arch = "wasm32")]
use crate::{Mrb, Value};

#[cfg(target_arch = "wasm32")]
impl Mrb {
    /// `mrb_get_args(mrb, "o", &val)` — read a single positional
    /// argument as a [`Value`].
    pub fn get_args_o(&self) -> Value {
        let mut raw = sys::mrb_value::zeroed();
        // SAFETY: `self` is alive by the `&self` borrow; `&mut raw`
        // is a valid `*mut mrb_value`; the `"o"` format writes
        // exactly one cell.
        unsafe {
            sys::mrb_get_args(
                self.as_ptr(),
                crate::cstr!("o"),
                &mut raw as *mut sys::mrb_value,
            );
        }
        Value::from_raw(raw)
    }

    /// `mrb_get_args(mrb, "*", &argv, &argc)` — read the rest array
    /// as a borrowed slice into the call frame.
    pub fn get_args_rest(&self) -> &[Value] {
        let mut argv: *const sys::mrb_value = core::ptr::null();
        let mut argc: core::ffi::c_int = 0;
        // SAFETY: as `get_args_o`; the `"*"` format writes the argv
        // pointer + length pair.
        unsafe {
            sys::mrb_get_args(
                self.as_ptr(),
                crate::cstr!("*"),
                &mut argv as *mut *const sys::mrb_value,
                &mut argc as *mut core::ffi::c_int,
            );
        }
        slice_from_argv(argv, argc)
    }

    /// `mrb_get_args(mrb, "n*", &sym, &argv, &argc)` — read a leading
    /// symbol followed by a rest array.
    pub fn get_args_n_rest(&self) -> (sys::mrb_sym, &[Value]) {
        let mut sym: sys::mrb_sym = 0;
        let mut argv: *const sys::mrb_value = core::ptr::null();
        let mut argc: core::ffi::c_int = 0;
        // SAFETY: as `get_args_o`.
        unsafe {
            sys::mrb_get_args(
                self.as_ptr(),
                crate::cstr!("n*"),
                &mut sym as *mut sys::mrb_sym,
                &mut argv as *mut *const sys::mrb_value,
                &mut argc as *mut core::ffi::c_int,
            );
        }
        (sym, slice_from_argv(argv, argc))
    }

    /// `mrb_get_args(mrb, "io", &n, &val)` — read an integer followed
    /// by an object.
    pub fn get_args_io(&self) -> (core::ffi::c_int, Value) {
        let mut n: core::ffi::c_int = 0;
        let mut raw = sys::mrb_value::zeroed();
        // SAFETY: as `get_args_o`.
        unsafe {
            sys::mrb_get_args(
                self.as_ptr(),
                crate::cstr!("io"),
                &mut n as *mut core::ffi::c_int,
                &mut raw as *mut sys::mrb_value,
            );
        }
        (n, Value::from_raw(raw))
    }
}

/// Cast a `mrb_get_args` rest-form `(*const mrb_value, c_int)` pair
/// into a borrowed `&[Value]`. mruby may set the pointer to NULL when
/// the rest count is zero; reading `len` bytes from NULL would be UB,
/// so the helper folds that into an empty slice.
///
/// The slice's lifetime is bound by the caller's `&self` borrow on
/// [`Mrb`] (the call frame that produced argv).
#[cfg(target_arch = "wasm32")]
#[inline]
fn slice_from_argv<'a>(argv: *const sys::mrb_value, argc: core::ffi::c_int) -> &'a [Value] {
    if argc > 0 && !argv.is_null() {
        // SAFETY: Value is `#[repr(transparent)]` over mrb_value;
        // mruby owns the buffer for the duration of the call frame
        // which outlives this borrow.
        unsafe { core::slice::from_raw_parts(argv as *const Value, argc as usize) }
    } else {
        &[]
    }
}
