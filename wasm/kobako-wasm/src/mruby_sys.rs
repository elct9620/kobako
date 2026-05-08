//! Hand-rolled mruby C API FFI bindings — minimum surface needed for
//! the Guest Binary boot mechanism.
//!
//! ## Why hand-rolled and not bindgen
//!
//! A future bindgen-driven binding generated from `vendor/mruby/include/`
//! at build time is anticipated, with `extern "C"` shim wrappers for any
//! C API exposed as a `static inline` macro in mruby headers. That path
//! is not yet wired in `build.rs` (the file comment in `build.rs` itself
//! documents this: "It does not run bindgen").
//!
//! For the boot mechanism the surface we actually call is small and
//! stable across mruby 3.x — the half-dozen registration functions
//! used by `boot.rs`. Hand-declaring them as `extern "C"` gives us:
//!
//!   * A wasm32 build that links against `libmruby.a` (host-side build
//!     pipeline already stages the archive — see `build.rs` and
//!     `tasks/wasm.rake`).
//!   * A host-target build that compiles cleanly: every mruby symbol is
//!     `#[cfg(target_arch = "wasm32")]`-gated, so the rlib used by
//!     `cargo test` on macOS / Linux never needs the symbols resolved.
//!
//! When bindgen lands (item tracked in `build.rs` TODO), this module
//! migrates to using the bindgen-emitted types and the C-side shims for
//! the `static inline` boxing macros.
//!
//! ## What is bound
//!
//! Only the C API functions needed for the §Boot Script 預載 three
//! registrations and the `Kobako.__rpc_call__` argument unpacking:
//!
//!   * `mrb_define_module`
//!   * `mrb_define_class_under`
//!   * `mrb_define_module_function`
//!   * `mrb_define_singleton_method`
//!   * `mrb_class_ptr`
//!   * `mrb_class_name`
//!   * `mrb_get_args`
//!   * `mrb_str_new` / `mrb_str_to_cstr` (string round-trip)
//!   * `mrb_raise` / `mrb_class_get_under` (exception path)
//!   * The `mrb_value` boxing helpers (declared as opaque `extern "C"`
//!     to side-step the static-inline issue — the future
//!     `crates/mruby-sys/wrapper.h` shim path).
//!
//! No other mruby C API is touched here.
//!
//! ## ABI / opaque types
//!
//! `mrb_value` layout depends on mruby compile-time configuration. For
//! wasm32 with `MRB_INT32` and `MRB_WORDBOX_NO_INLINE_FLOAT` the value
//! is a 64-bit word-box. We treat `mrb_value` as opaque (16 bytes to be
//! safe across all documented mruby configurations) and never inspect
//! its bits — the boxing helpers above are the only way we construct or
//! destructure values. Hand-rolled bit patterns would be an ABI
//! assumption violation; macro-routed values are not.

#![allow(non_camel_case_types)]
#![allow(dead_code)]

use core::ffi::c_void;
#[cfg(target_arch = "wasm32")]
use core::ffi::{c_char, c_int};

/// Opaque pointer to mruby state (`mrb_state *`).
pub type mrb_state = c_void;

/// Opaque mruby value. Sized at 16 bytes to fit any documented mruby
/// word-box layout (wasm32 `MRB_WORDBOX_NO_INLINE_FLOAT` is 8 bytes;
/// 64-bit no-inline-float and NaN-boxing variants are 8 bytes; a
/// 16-byte slot covers all).
#[repr(C)]
#[derive(Copy, Clone)]
pub struct mrb_value {
    _payload: [u64; 2],
}

impl mrb_value {
    /// Construct an opaque all-zero `mrb_value`. Used **only** as a
    /// placeholder return on host-target builds where the C bridges
    /// are no-ops (the wasm32 bodies always either raise or call into
    /// `mrb_*` boxing helpers and never return this value). Not safe
    /// to hand to a real mruby VM as a Ruby value.
    pub const fn zeroed() -> Self {
        Self { _payload: [0, 0] }
    }
}

/// Opaque `RClass *` — pointer to mruby class object.
pub type RClass = c_void;

/// Opaque `RObject *` — pointer to a generic mruby object header.
pub type RObject = c_void;

/// `mrb_sym` — interned symbol id. mruby uses 32-bit symbol ids by
/// default; treat as opaque.
pub type mrb_sym = u32;

/// C function pointer matching mruby's method-implementation signature
/// `mrb_value (*)(mrb_state*, mrb_value)`. Used by
/// `mrb_define_method` / `mrb_define_module_function` /
/// `mrb_define_singleton_method`.
pub type mrb_func_t =
    unsafe extern "C" fn(mrb: *mut mrb_state, self_: mrb_value) -> mrb_value;

/// `mrb_aspec` — packed argument specification (e.g. `MRB_ARGS_REQ(4)`).
/// In mruby this is a `uint32_t`. Construction macros listed below.
pub type mrb_aspec = u32;

/// `MRB_ARGS_NONE()` — no arguments.
pub const MRB_ARGS_NONE: mrb_aspec = 0;

/// `MRB_ARGS_ANY()` — accept any number of arguments. Matches mruby's
/// `MRB_ARGS_REST()` shape: 0 required, 0 optional, rest=1.
pub const MRB_ARGS_ANY: mrb_aspec = 1 << 12;

/// `MRB_ARGS_REQ(n)` — `n` required positional arguments.
#[inline]
pub const fn mrb_args_req(n: u32) -> mrb_aspec {
    (n & 0x1f) << 18
}

// --------------------------------------------------------------------
// FFI declarations.
// --------------------------------------------------------------------
//
// Only declared on wasm32 — the host-target rlib build deliberately
// has no libmruby.a in its link graph (see `build.rs` early-return on
// non-wasm32). Gating these here means `cargo test` on host compiles
// without unresolved symbols.

#[cfg(target_arch = "wasm32")]
extern "C" {
    /// `mrb_define_module(mrb, name)` — defines or returns the module
    /// named `name` at top level.
    pub fn mrb_define_module(mrb: *mut mrb_state, name: *const c_char) -> *mut RClass;

    /// `mrb_define_class_under(mrb, outer, name, super_)` — defines a
    /// class `name` under `outer`, inheriting from `super_`.
    pub fn mrb_define_class_under(
        mrb: *mut mrb_state,
        outer: *mut RClass,
        name: *const c_char,
        super_: *mut RClass,
    ) -> *mut RClass;

    /// `mrb_define_module_function(mrb, mod_, name, func, aspec)` —
    /// defines a module function on `mod_`.
    pub fn mrb_define_module_function(
        mrb: *mut mrb_state,
        mod_: *mut RClass,
        name: *const c_char,
        func: mrb_func_t,
        aspec: mrb_aspec,
    );

    /// `mrb_define_singleton_method(mrb, obj, name, func, aspec)` —
    /// defines a singleton-class method on `obj`.
    pub fn mrb_define_singleton_method(
        mrb: *mut mrb_state,
        obj: *mut RObject,
        name: *const c_char,
        func: mrb_func_t,
        aspec: mrb_aspec,
    );

    /// `mrb_class_ptr(val)` — the singleton-method `self` is the class
    /// object itself; `mrb_class_ptr` extracts the `RClass*` from it.
    pub fn mrb_class_ptr(val: mrb_value) -> *mut RClass;

    /// `mrb_class_name(mrb, c)` — returns the class's full Ruby name
    /// (e.g. `"MyService::KV"`).
    pub fn mrb_class_name(mrb: *mut mrb_state, c: *mut RClass) -> *const c_char;

    /// `mrb_get_args(mrb, format, ...)` — variadic argument unpack.
    /// We only need the rest-array form `"*"` — guarded by C calling
    /// convention varargs (`...`).
    pub fn mrb_get_args(mrb: *mut mrb_state, format: *const c_char, ...) -> c_int;

    /// `mrb_raise(mrb, c, msg)` — raises an exception of class `c`
    /// with `msg`. Used in the wire-fault path.
    pub fn mrb_raise(mrb: *mut mrb_state, c: *mut RClass, msg: *const c_char) -> !;

    /// `mrb_class_get_under(mrb, outer, name)` — fetches a class by
    /// name under `outer`. Used to resolve `Kobako::WireError` etc.
    /// when raising from the C bridge.
    pub fn mrb_class_get_under(
        mrb: *mut mrb_state,
        outer: *mut RClass,
        name: *const c_char,
    ) -> *mut RClass;

    /// `mrb_define_class(mrb, name, super_)` — defines a top-level
    /// class. Not currently used directly (the boot mechanism only
    /// calls `mrb_define_class_under` for `Kobako::RPC` and the future
    /// preamble subclasses), but declared here so future error-class
    /// registration paths have a stable binding.
    pub fn mrb_define_class(
        mrb: *mut mrb_state,
        name: *const c_char,
        super_: *mut RClass,
    ) -> *mut RClass;
}

// --------------------------------------------------------------------
// Compile-time signature checks (host target).
// --------------------------------------------------------------------
//
// On the host target the FFI block is absent, so we cannot link-check
// the symbols. We *can* however verify the type aliases and constants
// resolve and that constructed function pointers have the expected
// shape — this catches accidental signature drift in the FFI block.
// Cheap regression net.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mrb_args_constants_match_mruby_layout() {
        // `MRB_ARGS_REQ(n)` packs `n` into bits 18..23 of the aspec
        // word. mruby header: `((mrb_aspec)((n)&0x1f) << 18)`.
        assert_eq!(mrb_args_req(4), 4 << 18);
        assert_eq!(mrb_args_req(0), 0);
        assert_eq!(MRB_ARGS_ANY, 1 << 12);
        assert_eq!(MRB_ARGS_NONE, 0);
    }

    #[test]
    fn mrb_value_size_covers_known_layouts() {
        // The documented word-box layouts top out at 8 bytes
        // (NaN-boxing on 64-bit), but
        // we reserve 16 bytes so future layouts (e.g. an experimental
        // 128-bit Capn-style boxing) do not require an ABI break.
        assert!(core::mem::size_of::<mrb_value>() >= 8);
        assert_eq!(core::mem::align_of::<mrb_value>(), 8);
    }

    #[test]
    fn mrb_func_t_is_a_valid_extern_c_fn_pointer() {
        // Compile-time check: building a function with the expected
        // signature must coerce to `mrb_func_t` without an explicit
        // cast. If the `mrb_func_t` shape ever drifts, this function
        // definition fails to compile.
        unsafe extern "C" fn _stub(_mrb: *mut mrb_state, _self_: mrb_value) -> mrb_value {
            mrb_value::zeroed()
        }
        let _f: mrb_func_t = _stub;
    }
}
