//! mruby-sys — bindgen-driven mruby C API FFI surface.
//!
//! This crate is the boundary between the typed `mruby` wrapper and
//! `libmruby.a`. The entire FFI surface comes from `bindgen` at
//! build time:
//!
//!   * `src/wrapper.h` is the bindgen entry header. It includes the
//!     mruby header subset the guest calls and adds a small set of
//!     `static inline` wrappers around mruby's function-like macros
//!     (`RSTRING_PTR` / `RSTRING_LEN`, `mrb_obj_ptr`,
//!     `mrb_gc_arena_save` / `_restore`) and unexported helpers
//!     (`mrb_proc_new`) that bindgen cannot reach directly.
//!   * `build.rs::run_bindgen` emits the Rust bindings into
//!     `$OUT_DIR/bindings.rs` and the static-fn trampolines into
//!     `$OUT_DIR/mruby_static_wrappers.c`. The trampoline file is
//!     the single C translation unit the crate compiles — no
//!     hand-written `.c` shims live in `src/` any more.
//!
//! See `build.rs::run_bindgen` for the three documented
//! workarounds (`-fvisibility=default` for rust-bindgen #751,
//! `opaque_type("mrb_gc")` for the bitfield mis-pack, file-level
//! allowlist over name-regex).
//!
//! ## Why bindgen runs from inside this crate
//!
//! Confining the bindgen call here keeps libclang a sys-only build
//! dependency. The downstream `mruby` wrapper (and through it,
//! `kobako-wasm`) consumes this crate as a path dependency and
//! never sees bindgen — so the cost of staging libclang sits in
//! one place, against one well-defined header set (`src/wrapper.h`),
//! instead of leaking into every consumer build.
//!
//! ## No typed wrappers here
//!
//! The typed `Value` / `Class` / `Array` / `Hash` newtypes, the
//! `Mrb` / `Ccontext` RAII wrappers, and the `IntoValue` /
//! `FromValue` / `Format` trait seams all live in the sibling
//! `mruby` crate. This crate stays a pure FFI surface: bindgen
//! output, the `mrb_func_t` typed-fn alias, the
//! `mrb_value::zeroed()` constant, the `mrb_args_*` aspec encoders,
//! the `mrb_object_class` raw-state accessor, and the ABI const
//! assertions that catch a vendored-mruby layout drift at compile
//! time.
//!
//! ## ABI / opaque types
//!
//! `mrb_value` layout depends on mruby compile-time configuration.
//! For wasm32 with `MRB_INT32` and `MRB_WORDBOX_NO_INLINE_FLOAT`
//! the value is a 32-bit word-box (`struct { uintptr_t w }` where
//! `uintptr_t` is 4 bytes). The `build.rs` clang invocation mirrors
//! those defines so bindgen sees the same layout libmruby.a was
//! built with. The const assertions below pin the size / align at
//! compile time — any future vendor bump that drifts the layout
//! fails to compile rather than silently breaking the ABI.

#![allow(non_camel_case_types)]
#![allow(non_upper_case_globals)]
#![allow(non_snake_case)]

#[cfg(not(target_arch = "wasm32"))]
use core::ffi::c_void;

// --------------------------------------------------------------------
// bindgen-generated FFI surface (wasm32 only).
// --------------------------------------------------------------------
//
// On the host target the FFI block is absent. Tests that target the
// pure-Rust unit suite (codec / outcome / transport envelope) still
// need `mrb_value` / `mrb_state` / `RClass` etc. to resolve as
// types — the stub aliases below cover that.
//
// The generated `bindings.rs` is `include!`-d into a private
// submodule so the `#![allow(clippy::all)]` / `#![allow(warnings)]`
// scope contains the auto-generated bitfield accessors (which use
// `unsafe { transmute(...) }` patterns clippy flags). The `pub use`
// re-exports every name at the crate root, keeping the consumer
// import path unchanged.

#[cfg(target_arch = "wasm32")]
#[allow(clippy::all)]
#[allow(warnings)]
mod bindings {
    // `mrb_func_t` is blocklisted in bindgen so consumers see the
    // typed-`mrb_value` alias declared at the crate root. The
    // generated bindings still reference the bare name in function
    // signatures (e.g. `mrb_define_method`'s `func` parameter);
    // pull the parent alias into scope so those references resolve.
    use super::mrb_func_t;
    include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
}

#[cfg(target_arch = "wasm32")]
pub use bindings::*;

#[cfg(target_arch = "wasm32")]
impl mrb_value {
    /// All-zero `mrb_value`. On wasm32 with the kobako mruby
    /// configuration this matches `mrb_nil_value()` (MRB_Qnil = 0).
    /// Out-parameter initialisers (`mrb_get_args` writes to it) use
    /// this; callers that need a guaranteed nil should prefer the
    /// `Value::nil` accessor in the `mruby` wrapper which reads
    /// through mruby's helper.
    pub const fn zeroed() -> Self {
        Self { w: 0 }
    }
}

// Compile-time pin on the wasm32 mrb_value layout. Catches a future
// bindgen / build_config drift before it silently breaks ABI.
#[cfg(target_arch = "wasm32")]
const _: () = assert!(
    core::mem::size_of::<mrb_value>() == 4,
    "mrb_value size diverged from MRB_WORDBOX_NO_INLINE_FLOAT layout"
);
#[cfg(target_arch = "wasm32")]
const _: () = assert!(
    core::mem::align_of::<mrb_value>() == 4,
    "mrb_value alignment diverged from MRB_WORDBOX_NO_INLINE_FLOAT layout"
);

// `Mrb::pending_exc` and `Mrb::load_bytecode`'s exception
// synthesiser (in the `mruby` wrapper crate) read / write
// `mrb_state.exc` through bindgen's struct accessor. Pin the
// field's offset so a future bindgen run or mruby vendor bump that
// shifts it fails at compile time rather than silently reading the
// wrong slot. The field sits after `jmp` / `c` / `root_c` /
// `globals` (four pointer-sized fields); `mrb_gc` (which carries
// the bitfield workaround) lives further down the struct, so the
// bitfield mis-pack does not affect this offset.
#[cfg(target_arch = "wasm32")]
const _: () = assert!(
    core::mem::offset_of!(mrb_state, exc) == 4 * core::mem::size_of::<*const core::ffi::c_void>(),
    "mrb_state.exc offset diverged from the vendored mruby layout — \
     the pending-exception helpers read this field directly"
);

/// Read `mrb->object_class` from a raw `*mut mrb_state`. Companion
/// accessor for code paths that hold a raw pointer rather than an
/// `Mrb` borrow — currently the `install_*` helpers in
/// `kobako-wasm/src/kobako/install.rs` which are themselves called
/// with a raw `*mut mrb_state` from `Kobako::install`.
///
/// Prefer the `mruby` wrapper's `Mrb::object_class` when an `Mrb`
/// borrow is in scope.
///
/// # Safety
///
/// `mrb` must be a live mruby state. The returned pointer aliases the
/// state's interior `object_class` field; it remains valid for the
/// state's lifetime and must not be passed to `mrb_close` or freed.
#[cfg(target_arch = "wasm32")]
#[inline]
pub unsafe fn mrb_object_class(mrb: *mut mrb_state) -> *mut RClass {
    unsafe { (*mrb).object_class }
}

// --------------------------------------------------------------------
// Host-target placeholders.
// --------------------------------------------------------------------
//
// bindgen does not run on non-wasm32 (see `build.rs`'s early return),
// so the rlib needs hand-written placeholders for the type names the
// consumer's pure-Rust unit tests reference. These types are not
// link-checked against any C definition; they exist only to make
// signatures compile so `mrb_func_t` shape tests and similar fixtures
// keep running on host CI.

#[cfg(not(target_arch = "wasm32"))]
pub type mrb_state = c_void;
#[cfg(not(target_arch = "wasm32"))]
pub type RClass = c_void;
#[cfg(not(target_arch = "wasm32"))]
pub type RObject = c_void;
#[cfg(not(target_arch = "wasm32"))]
pub type mrb_sym = u32;
#[cfg(not(target_arch = "wasm32"))]
pub type mrb_aspec = u32;
#[cfg(not(target_arch = "wasm32"))]
pub type mrb_bool = bool;
#[cfg(not(target_arch = "wasm32"))]
pub type mrb_int = i32;
#[cfg(not(target_arch = "wasm32"))]
pub type mrb_float = f64;
#[cfg(not(target_arch = "wasm32"))]
pub type mrb_ccontext = c_void;

#[cfg(not(target_arch = "wasm32"))]
#[repr(C)]
#[derive(Copy, Clone)]
pub struct mrb_value {
    _payload: [u64; 2],
}
#[cfg(not(target_arch = "wasm32"))]
impl mrb_value {
    /// All-zero `mrb_value`. On the host target this produces a
    /// zeroed 16-byte placeholder.
    pub const fn zeroed() -> Self {
        Self { _payload: [0, 0] }
    }
}

// --------------------------------------------------------------------
// Typed function-pointer alias.
// --------------------------------------------------------------------
//
// `mrb_func_t` is blocklisted in the bindgen builder so consumers can
// import the typed shape declared here. The signature uses the raw
// `mrb_value` directly; the `mruby` wrapper crate's `Value` newtype
// is `#[repr(transparent)]` over `mrb_value`, so a bridge declared
// with `Value` parameters and return type coerces to this alias
// without an `Option`-wrapping cast.

/// C function pointer matching mruby's method-implementation signature
/// `mrb_value (*)(mrb_state*, mrb_value)`. Used by `mrb_define_method`
/// and `mrb_define_singleton_method`.
pub type mrb_func_t = unsafe extern "C" fn(mrb: *mut mrb_state, self_: mrb_value) -> mrb_value;

// --------------------------------------------------------------------
// Argument-spec encoders.
// --------------------------------------------------------------------
//
// mruby spells these as the function-like macros MRB_ARGS_NONE() /
// MRB_ARGS_ANY() / MRB_ARGS_REQ(n); bindgen cannot expand macros, so
// the `mrb_args_*_func` static-inline shims in `wrapper.h` emit the
// bit packing from mruby's own header (reached through bindgen's
// `wrap_static_fns` trampolines). These safe wrappers forward to the
// trampolines so method-registration sites keep a const-like call
// shape without an `unsafe` block, and the encoding can never desync
// from a mruby vendor bump the way a Rust-side bit-packing mirror
// could.

/// `MRB_ARGS_NONE()` — no arguments.
#[cfg(target_arch = "wasm32")]
#[inline]
pub fn mrb_args_none() -> mrb_aspec {
    // SAFETY: pure value computation; touches no mrb_state.
    unsafe { mrb_args_none_func() }
}

/// `MRB_ARGS_ANY()` — accept any number of arguments.
#[cfg(target_arch = "wasm32")]
#[inline]
pub fn mrb_args_any() -> mrb_aspec {
    // SAFETY: as `mrb_args_none`.
    unsafe { mrb_args_any_func() }
}

/// `MRB_ARGS_REQ(n)` — `n` required positional arguments.
#[cfg(target_arch = "wasm32")]
#[inline]
pub fn mrb_args_req(n: u32) -> mrb_aspec {
    // SAFETY: as `mrb_args_none`.
    unsafe { mrb_args_req_func(n) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mrb_value_size_covers_known_layouts() {
        // The documented word-box layouts top out at 8 bytes
        // (NaN-boxing on 64-bit), but we reserve 16 bytes on host so
        // future layouts do not require an ABI break.
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
