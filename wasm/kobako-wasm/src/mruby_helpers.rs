//! Lightweight Rust-side conveniences over the mruby C API.
//!
//! This module adds **inherent methods on `mrb_value`** for the three
//! patterns that appear dozens of times across the guest binary, plus
//! two free items for static C-string handling. The shape mirrors how
//! the [`magnus`](https://docs.rs/magnus) crate exposes `Value` methods
//! for the CRuby C API: value-centric operations are methods on the
//! value, byte-string utilities are free items.
//!
//! ## Methods on `mrb_value`
//!
//! All three are `unsafe` — they forward to FFI calls that require a
//! live `mrb_state *` produced the same VM as `self`.
//!
//!   * [`mrb_value::classname`] — Ruby class name of this value.
//!   * [`mrb_value::to_string`] — coerce to Rust `String` via
//!     `Object#to_s`. Works on any value type (Strings included —
//!     `String#to_s` is idempotent).
//!   * [`mrb_value::call`] — invoke `self.method_name(args...)` via
//!     `mrb_funcall_argv` (non-variadic, slice-based).
//!
//! ## Free items
//!
//! Generic NUL-terminated C-string helpers — they take or produce
//! `*const c_char` and have no dependency on `mrb_state`.
//!
//!   * [`cstr_ptr`] — coerce a NUL-terminated `&[u8]` constant to
//!     `*const c_char`.
//!   * [`cstr!`] macro — compile-time NUL-terminate a string literal
//!     and return `*const c_char`.
//!
//! ## What is intentionally NOT here
//!
//! No typed `MString` / `MArray` / `MHash` newtype wrappers. The
//! wasm32 `mrb_value` word-box ABI is small enough that we keep
//! passing `mrb_value` directly. Methods on the FFI type itself give
//! us most of the ergonomic win without committing to a typed value
//! framework — the same trade-off discussed at the top of
//! `mruby_sys.rs`.

use crate::mruby_sys as sys;

/// Compile-time NUL-terminated C-string literal pointer.
///
/// `cstr!("name")` expands to `concat!("name", "\0").as_ptr() as *const c_char`,
/// avoiding the noisy hand-written `b"name\0".as_ptr() as *const core::ffi::c_char`
/// pattern at every FFI call site.
#[macro_export]
macro_rules! cstr {
    ($s:expr) => {
        concat!($s, "\0").as_ptr() as *const core::ffi::c_char
    };
}

/// Coerce a NUL-terminated byte slice to `*const c_char`. Used for the
/// top-of-file `const X: &[u8] = b"...\0"` declarations that already
/// carry their NUL terminator — `cstr_ptr(KOBAKO_NAME)` reads cleaner
/// than `KOBAKO_NAME.as_ptr() as *const core::ffi::c_char`.
///
/// The caller must guarantee `b` ends with `0u8` — debug builds assert.
#[inline]
pub const fn cstr_ptr(b: &[u8]) -> *const core::ffi::c_char {
    debug_assert!(!b.is_empty());
    debug_assert!(b[b.len() - 1] == 0);
    b.as_ptr() as *const core::ffi::c_char
}

#[cfg(target_arch = "wasm32")]
impl sys::mrb_value {
    /// Returns the Ruby class name of this value as a borrowed
    /// `&'static str`, or `""` if mruby returns NULL.
    ///
    /// The returned slice points into mruby's interned class-name
    /// storage, which lives for the duration of the `mrb_state`. We
    /// expose it as `&'static str` for ergonomic comparisons; callers
    /// that need to retain the name across a GC point should
    /// `.to_string()` it.
    ///
    /// # Safety
    ///
    /// `mrb` must be a live `mrb_state *` and `self` must have been
    /// produced by the same VM.
    #[inline]
    pub unsafe fn classname(self, mrb: *mut sys::mrb_state) -> &'static str {
        let ptr = sys::mrb_obj_classname(mrb, self);
        if ptr.is_null() {
            return "";
        }
        core::ffi::CStr::from_ptr(ptr).to_str().unwrap_or("")
    }

    /// Coerce to a Rust `String` by calling `Object#to_s` and copying
    /// the bytes. Works on any value type — `String#to_s` is
    /// idempotent on mruby Strings, so the redundant call is cheap and
    /// keeps a single conversion entry point.
    ///
    /// Equivalent to the inlined sequence:
    ///
    /// ```text
    /// mrb_funcall(.., "to_s", 0) → mrb_str_to_cstr → CStr::from_ptr
    ///                            → to_str → to_string
    /// ```
    ///
    /// Returns `String::new()` on any failure (NULL pointer, non-UTF-8
    /// content).
    ///
    /// # Safety
    ///
    /// `mrb` must be a live `mrb_state *` and `self` must have been
    /// produced by the same VM.
    #[inline]
    pub unsafe fn to_string(self, mrb: *mut sys::mrb_state) -> String {
        let s_val = self.call(mrb, cstr!("to_s"), &[]);
        let ptr = sys::mrb_str_to_cstr(mrb, s_val);
        if ptr.is_null() {
            return String::new();
        }
        core::ffi::CStr::from_ptr(ptr).to_str().unwrap_or("").to_string()
    }

    /// Invoke `self.method_name(args...)` via the non-variadic
    /// `mrb_funcall_argv`. The method name is interned each call;
    /// mruby's symbol table makes this cheap.
    ///
    /// `name_cstr` must be a NUL-terminated byte slice pointer —
    /// produce one with the [`cstr!`] macro for inline literals or
    /// with [`cstr_ptr`] for a named constant. `args` may be empty for
    /// arity-0 calls.
    ///
    /// The wrapper exists so call sites stop reaching for the variadic
    /// `sys::mrb_funcall` directly (every variadic FFI call in Rust is
    /// an `unsafe` footgun the type checker can't help with).
    ///
    /// # Examples (caller code shape)
    ///
    /// ```ignore
    /// val.call(mrb, cstr!("to_s"), &[]);
    /// str_val.call(mrb, cstr!("getbyte"), &[idx]);
    /// ```
    ///
    /// # Safety
    ///
    /// `mrb` must be a live `mrb_state *`. `self` and every `args`
    /// entry must have been produced by the same VM.
    #[inline]
    pub unsafe fn call(
        self,
        mrb: *mut sys::mrb_state,
        name_cstr: *const core::ffi::c_char,
        args: &[sys::mrb_value],
    ) -> sys::mrb_value {
        let sym = sys::mrb_intern_cstr(mrb, name_cstr);
        sys::mrb_funcall_argv(
            mrb,
            self,
            sym,
            args.len() as core::ffi::c_int,
            args.as_ptr(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cstr_macro_appends_nul_terminator() {
        let p = cstr!("hello");
        let cs = unsafe { core::ffi::CStr::from_ptr(p) };
        assert_eq!(cs.to_str().unwrap(), "hello");
    }

    #[test]
    fn cstr_ptr_accepts_nul_terminated_bytes() {
        const NAME: &[u8] = b"Kobako\0";
        let p = cstr_ptr(NAME);
        let cs = unsafe { core::ffi::CStr::from_ptr(p) };
        assert_eq!(cs.to_str().unwrap(), "Kobako");
    }

    #[test]
    fn cstr_macro_handles_empty_string() {
        let p = cstr!("");
        let cs = unsafe { core::ffi::CStr::from_ptr(p) };
        assert_eq!(cs.to_str().unwrap(), "");
    }
}
