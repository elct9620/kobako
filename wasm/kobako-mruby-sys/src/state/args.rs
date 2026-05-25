//! `mrb_get_args` shape-typed dispatch on [`Mrb`].
//!
//! mruby's `mrb_get_args` is a variadic C function whose format string
//! drives heterogeneous out-parameters at runtime. Rust cannot express
//! that signature directly: a single Rust function cannot vary its
//! return type with a runtime format string, and `extern "C"` variadics
//! force every call site into hand-counted `unsafe` plumbing.
//!
//! The trade is to lift the format string to the *type* level. Each
//! format becomes a zero-sized marker type implementing [`Format`];
//! [`Mrb::get_args`] is the single safe entry point that
//! monomorphises the FFI call against `F::FMT` and returns the typed
//! tuple from `F::Output`.
//!
//!   - [`format::O`]          — `"o"`   → single positional
//!   - [`format::Rest`]       — `"*"`   → rest array borrowed from the
//!     call frame
//!   - [`format::NRest`]      — `"n*"`  → symbol + rest array
//!   - [`format::NRestBlock`] — `"n*&"` → symbol + rest array + block
//!     slot
//!   - [`format::Io`]         — `"io"`  → integer + object
//!
//! Rest-form variants borrow the call frame's argv buffer; the
//! lifetime is tied to `&self`, which the bridge body holds for the
//! duration of the C call. mruby may set the rest pointer to NULL
//! when the rest count is zero — `slice_from_argv` folds that into
//! an empty `&[Value]` so callers do not have to gate on NULL.
//!
//! ## Why a trait rather than per-method wrappers
//!
//! The previous shape was four inherent methods on [`Mrb`] — one per
//! format string. That worked for a closed set, but every new format
//! widened the [`Mrb`] surface and duplicated the variadic FFI dance.
//! The trait pattern flips the axis: format identity moves to a ZST,
//! the dispatch surface collapses to a single `get_args::<F>()`, and
//! adding a fifth format means adding a struct + impl — not editing
//! `impl Mrb`. The same pattern is the right template for any other
//! capability cluster that currently lives as fan-of-methods on
//! [`Mrb`] (`Define`, `Build`, etc.) once a similar combinatorial
//! pressure shows up.
//!
//! ## Extending with a new format
//!
//! Add a marker ZST under [`format`] and implement [`Format`]:
//!
//! ```ignore
//! use kobako_mruby_sys::{Format, Mrb, Value};
//!
//! pub struct S;
//! impl Format for S {
//!     type Output<'a> = Value;
//!     const FMT: &'static core::ffi::CStr = c"S";
//!     fn read(mrb: &Mrb) -> Self::Output<'_> {
//!         // mrb_get_args(mrb, "S", &out) — see `format::O` for the pattern
//!         # unimplemented!()
//!     }
//! }
//! ```

#[cfg(target_arch = "wasm32")]
use crate as sys;
#[cfg(target_arch = "wasm32")]
use crate::{Mrb, Value};

/// Type-level marker for a single `mrb_get_args` format string.
///
/// Implementors are zero-sized structs (see [`format`]) whose
/// [`Format::FMT`] supplies the mruby format and whose
/// [`Format::Output`] names the typed return shape. The GAT lifetime
/// `'a` carries the borrow from the call-frame argv slot for
/// rest-form formats; immediate formats leave it unused.
///
/// New implementors should monomorphise the `mrb_get_args` call inside
/// [`Format::read`] against [`Format::FMT`] — see `format::O` for the
/// minimal pattern.
#[cfg(target_arch = "wasm32")]
pub trait Format {
    /// Typed shape returned by [`Format::read`]. The `'a` lifetime is
    /// the borrow on the call-frame argv slot for rest-form formats;
    /// immediate formats leave it unused.
    type Output<'a>;

    /// mruby format string (e.g. `c"o"`, `c"n*"`). Static-lifetime
    /// `&CStr` so the format byte sequence is interned at compile
    /// time alongside the impl.
    const FMT: &'static core::ffi::CStr;

    /// Read the call-frame argv against `Self::FMT` and project it
    /// into [`Format::Output`]. The body issues exactly one
    /// `mrb_get_args` call with the per-format out-parameter shape.
    fn read(mrb: &Mrb) -> Self::Output<'_>;
}

#[cfg(target_arch = "wasm32")]
impl Mrb {
    /// Read the call-frame argv using a [`Format`] marker. The
    /// monomorphised call expands to a single `mrb_get_args` against
    /// `F::FMT` and returns the typed tuple from `F::Output`.
    ///
    /// ```ignore
    /// use kobako_mruby_sys::format::{Io, Rest};
    /// let (fd, mode_val) = mrb.get_args::<Io>();
    /// let argv = mrb.get_args::<Rest>();
    /// ```
    #[inline]
    pub fn get_args<F: Format>(&self) -> F::Output<'_> {
        F::read(self)
    }
}

/// Zero-sized marker types implementing [`Format`]. Each marker maps
/// one mruby format string to a typed Rust return.
#[cfg(target_arch = "wasm32")]
pub mod format {
    use super::sys;
    use super::{slice_from_argv, Format, Mrb, Value};

    /// `mrb_get_args(mrb, "o", &val)` — read a single positional
    /// argument as a [`Value`].
    pub struct O;
    impl Format for O {
        type Output<'a> = Value;
        const FMT: &'static core::ffi::CStr = c"o";

        fn read(mrb: &Mrb) -> Value {
            let mut raw = sys::mrb_value::zeroed();
            // SAFETY: `mrb` is alive by the `&Mrb` borrow; `&mut raw`
            // is a valid `*mut mrb_value`; the `"o"` format writes
            // exactly one cell.
            unsafe {
                sys::mrb_get_args(
                    mrb.as_ptr(),
                    Self::FMT.as_ptr(),
                    &mut raw as *mut sys::mrb_value,
                );
            }
            Value::from_raw(raw)
        }
    }

    /// `mrb_get_args(mrb, "*", &argv, &argc)` — read the rest array
    /// as a borrowed slice into the call frame.
    pub struct Rest;
    impl Format for Rest {
        type Output<'a> = &'a [Value];
        const FMT: &'static core::ffi::CStr = c"*";

        fn read(mrb: &Mrb) -> &[Value] {
            let mut argv: *const sys::mrb_value = core::ptr::null();
            let mut argc: core::ffi::c_int = 0;
            // SAFETY: as `O::read`; the `"*"` format writes the argv
            // pointer + length pair.
            unsafe {
                sys::mrb_get_args(
                    mrb.as_ptr(),
                    Self::FMT.as_ptr(),
                    &mut argv as *mut *const sys::mrb_value,
                    &mut argc as *mut core::ffi::c_int,
                );
            }
            slice_from_argv(argv, argc)
        }
    }

    /// `mrb_get_args(mrb, "n*", &sym, &argv, &argc)` — read a leading
    /// symbol followed by a rest array.
    pub struct NRest;
    impl Format for NRest {
        type Output<'a> = (sys::mrb_sym, &'a [Value]);
        const FMT: &'static core::ffi::CStr = c"n*";

        fn read(mrb: &Mrb) -> (sys::mrb_sym, &[Value]) {
            let mut sym: sys::mrb_sym = 0;
            let mut argv: *const sys::mrb_value = core::ptr::null();
            let mut argc: core::ffi::c_int = 0;
            // SAFETY: as `O::read`.
            unsafe {
                sys::mrb_get_args(
                    mrb.as_ptr(),
                    Self::FMT.as_ptr(),
                    &mut sym as *mut sys::mrb_sym,
                    &mut argv as *mut *const sys::mrb_value,
                    &mut argc as *mut core::ffi::c_int,
                );
            }
            (sym, slice_from_argv(argv, argc))
        }
    }

    /// `mrb_get_args(mrb, "n*&", &sym, &argv, &argc, &block)` — read a
    /// leading symbol, then a rest array, then the block slot from the
    /// call frame. The `&` specifier produces a value copy of the block
    /// `mrb_value` without invoking `mrb_proc_copy`, so the captured
    /// block stays non-orphan
    /// (`vendor/mruby/src/class.c:1593-1604`). When the caller supplied
    /// no block the slot decodes as `mrb_nil`.
    pub struct NRestBlock;
    impl Format for NRestBlock {
        type Output<'a> = (sys::mrb_sym, &'a [Value], Value);
        const FMT: &'static core::ffi::CStr = c"n*&";

        fn read(mrb: &Mrb) -> (sys::mrb_sym, &[Value], Value) {
            let mut sym: sys::mrb_sym = 0;
            let mut argv: *const sys::mrb_value = core::ptr::null();
            let mut argc: core::ffi::c_int = 0;
            let mut block_raw = sys::mrb_value::zeroed();
            // SAFETY: as `O::read`; the `"n*&"` format writes the
            // leading symbol, the argv pointer + length pair, and a
            // single block-slot value.
            unsafe {
                sys::mrb_get_args(
                    mrb.as_ptr(),
                    Self::FMT.as_ptr(),
                    &mut sym as *mut sys::mrb_sym,
                    &mut argv as *mut *const sys::mrb_value,
                    &mut argc as *mut core::ffi::c_int,
                    &mut block_raw as *mut sys::mrb_value,
                );
            }
            (sym, slice_from_argv(argv, argc), Value::from_raw(block_raw))
        }
    }

    /// `mrb_get_args(mrb, "io", &n, &val)` — read an integer followed
    /// by an object. The `"i"` specifier writes an `mrb_int`, so the
    /// out-param is typed `sys::mrb_int` (not `c_int`) to match mruby's
    /// own width contract rather than coincide with it on the wasm32
    /// `MRB_INT32` build.
    pub struct Io;
    impl Format for Io {
        type Output<'a> = (sys::mrb_int, Value);
        const FMT: &'static core::ffi::CStr = c"io";

        fn read(mrb: &Mrb) -> (sys::mrb_int, Value) {
            let mut n: sys::mrb_int = 0;
            let mut raw = sys::mrb_value::zeroed();
            // SAFETY: as `O::read`.
            unsafe {
                sys::mrb_get_args(
                    mrb.as_ptr(),
                    Self::FMT.as_ptr(),
                    &mut n as *mut sys::mrb_int,
                    &mut raw as *mut sys::mrb_value,
                );
            }
            (n, Value::from_raw(raw))
        }
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
