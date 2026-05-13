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
//! used by `crate::kobako::bridges`. Hand-declaring them as `extern "C"`
//! gives us:
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
//! Only the C API functions needed for the three boot-script
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
//!   * `mrb_raise` / `mrb_class_get_under` / `mrb_class_get` /
//!     `mrb_module_get` (exception path + Kernel registration)
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

/// `mrb_bool` — mruby's boolean C type (unsigned char / u8).
#[cfg(target_arch = "wasm32")]
pub type mrb_bool = u8;

/// `mrb_protect_error_func` — function pointer type accepted by
/// `mrb_protect_error`. Receives `mrb` + `userdata` and returns an
/// `mrb_value`.
#[cfg(target_arch = "wasm32")]
pub type mrb_protect_error_func =
    unsafe extern "C" fn(mrb: *mut mrb_state, userdata: *mut c_void) -> mrb_value;

/// Opaque pointer to mruby state (`mrb_state *`).
pub type mrb_state = c_void;

/// Opaque mruby value. The layout is target-specific:
///
/// - **wasm32-wasip1** (production target): `mrb_value` is `struct { uintptr_t w }`
///   where `uintptr_t` is 4 bytes → `mrb_value` is exactly 4 bytes.
///   This is the `MRB_WORDBOX_NO_INLINE_FLOAT` configuration produced by
///   the kobako build config (`build_config/wasi.rb`).
///
/// - **host target** (macOS/Linux aarch64/x86_64, used for `cargo test`):
///   The mruby C API is not linked so the exact layout does not matter;
///   we use a 16-byte opaque placeholder that is large enough to cover any
///   documented layout and satisfies alignment requirements.
///
/// The wasm32 size is critical: if the Rust type is larger than the C type,
/// the Rust compiler emits sret-style calls (return via out-pointer) that
/// do not match the wasm32 C ABI where a 4-byte return fits in a register.
#[cfg(target_arch = "wasm32")]
#[repr(C)]
#[derive(Copy, Clone)]
pub struct mrb_value {
    pub w: u32,
}

#[cfg(not(target_arch = "wasm32"))]
#[repr(C)]
#[derive(Copy, Clone)]
pub struct mrb_value {
    _payload: [u64; 2],
}

impl mrb_value {
    /// Construct an opaque all-zero `mrb_value`. On wasm32 this produces
    /// `{ w: 0 }` which is mruby's `nil` value (MRB_Qnil = 0). On the host
    /// target this produces a zeroed 16-byte placeholder.
    pub const fn zeroed() -> Self {
        #[cfg(target_arch = "wasm32")]
        {
            Self { w: 0 }
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            Self { _payload: [0, 0] }
        }
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
pub type mrb_func_t = unsafe extern "C" fn(mrb: *mut mrb_state, self_: mrb_value) -> mrb_value;

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

    // NOTE: mrb_class_ptr is a C macro, not a real function:
    //   #define mrb_class_ptr(v) ((struct RClass*)(mrb_ptr(v)))
    // With MRB_WORDBOX_NO_INLINE_FLOAT + MRB_INT32 (wasm32 config),
    // mrb_ptr(val) resolves to the raw pointer stored in the lower 32 bits.
    // Use `val.w as *mut RClass` inline at bridge call sites; do NOT
    // declare it here as an extern "C" fn (that would produce a wasm
    // import for a symbol that doesn't exist as a real C function in
    // libmruby.a).

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

    /// `mrb_open()` — creates and initializes a new mruby interpreter
    /// state. Returns NULL on allocation failure. Called once at the
    /// start of every `__kobako_run` invocation.
    pub fn mrb_open() -> *mut mrb_state;

    /// `mrb_close(mrb)` — destroys the mruby state and frees all
    /// associated memory. Called at the end of `__kobako_run`.
    pub fn mrb_close(mrb: *mut mrb_state);

    /// `mrb_load_nstring(mrb, s, len)` — compiles and evaluates the
    /// Ruby source string `s[0..len]`. Returns the last expression
    /// value; sets `mrb->exc` on parse or runtime error.
    pub fn mrb_load_nstring(mrb: *mut mrb_state, s: *const c_char, len: usize) -> mrb_value;

    /// `mrb_obj_classname(mrb, obj)` — returns a pointer to the class
    /// name C string of `obj`. The pointer is owned by mruby and must
    /// not be freed.
    pub fn mrb_obj_classname(mrb: *mut mrb_state, obj: mrb_value) -> *const c_char;

    /// `mrb_funcall(mrb, val, name, argc, ...)` — variadic Ruby method
    /// call from C. Used to call `.message` on an exception value.
    /// The call frame is not protected — callers must ensure `mrb->exc`
    /// is already set as a known exception before calling this.
    ///
    /// Prefer `mrb_funcall_argv` (the non-variadic counterpart) when
    /// the call site has a fixed argv slice — it gives the Rust borrow
    /// checker something to verify and avoids variadic-FFI footguns.
    pub fn mrb_funcall(
        mrb: *mut mrb_state,
        val: mrb_value,
        name: *const c_char,
        argc: c_int,
        ...
    ) -> mrb_value;

    /// `mrb_funcall_argv(mrb, val, mid, argc, argv)` — non-variadic
    /// counterpart to `mrb_funcall`. Takes a pre-interned method
    /// symbol and an `argv` array pointer. Used by `mrb_value::call`
    /// (`crate::mruby::value`) so call sites stop reaching for the
    /// variadic `mrb_funcall`.
    pub fn mrb_funcall_argv(
        mrb: *mut mrb_state,
        val: mrb_value,
        mid: mrb_sym,
        argc: c_int,
        argv: *const mrb_value,
    ) -> mrb_value;

    /// `mrb_str_to_cstr(mrb, str)` — returns a NUL-terminated C string
    /// from an mruby String value. The pointer is valid until the next
    /// GC cycle; callers must copy before yielding control to mruby.
    pub fn mrb_str_to_cstr(mrb: *mut mrb_state, str: mrb_value) -> *mut c_char;

    /// `mrb_protect_error(mrb, body, userdata, error)` — calls `body`
    /// via a protected frame. On exception, `*error` is set to TRUE and
    /// the return value is the exception object. On success, `*error` is
    /// FALSE and the return value is `body`'s return value.
    pub fn mrb_protect_error(
        mrb: *mut mrb_state,
        body: mrb_protect_error_func,
        userdata: *mut c_void,
        error: *mut mrb_bool,
    ) -> mrb_value;

    /// `mrb_check_error(mrb)` — returns TRUE if `mrb->exc` is set, then
    /// clears it. Used after `mrb_load_nstring` to detect exceptions
    /// without accessing the struct field directly.
    pub fn mrb_check_error(mrb: *mut mrb_state) -> mrb_bool;

    /// `mrb_sym_name(mrb, sym)` — returns the C string name for a symbol.
    /// Used to extract the method name from `method_missing` args.
    pub fn mrb_sym_name(mrb: *mut mrb_state, sym: mrb_sym) -> *const c_char;

    /// `mrb_str_new_cstr(mrb, str)` — creates a new mruby String from a
    /// NUL-terminated C string.
    pub fn mrb_str_new_cstr(mrb: *mut mrb_state, s: *const c_char) -> mrb_value;

    /// `mrb_ary_entry(ary, offset)` — returns the element at `offset` in
    /// `ary`. No bounds checking on the C side; caller must ensure offset
    /// is in range.
    ///
    /// `offset` is `mrb_int` which on wasm32 (MRB_INT32 config) is a 32-bit
    /// signed integer.
    pub fn mrb_ary_entry(ary: mrb_value, offset: i32) -> mrb_value;

    /// `mrb_hash_keys(mrb, hash)` — returns an Array of the hash's keys.
    pub fn mrb_hash_keys(mrb: *mut mrb_state, hash: mrb_value) -> mrb_value;

    /// `mrb_hash_get(mrb, hash, key)` — returns the value for `key` in
    /// `hash`, or nil if not present.
    pub fn mrb_hash_get(mrb: *mut mrb_state, hash: mrb_value, key: mrb_value) -> mrb_value;

    /// `mrb_hash_p(mrb, obj)` — NOTE: this is a predicate macro in mruby,
    /// not a real C function. Checking is done via mrb_obj_classname
    /// comparison instead.

    /// `mrb_intern_cstr(mrb, str)` — interns a NUL-terminated C string
    /// as a symbol. Used to build string keys for mrb_hash_get.
    pub fn mrb_intern_cstr(mrb: *mut mrb_state, str: *const c_char) -> mrb_sym;

    /// `mrb_sym_str(mrb, sym)` — converts a symbol to its String representation.
    pub fn mrb_sym_str(mrb: *mut mrb_state, sym: mrb_sym) -> mrb_value;

    /// `mrb_str_new(mrb, p, len)` — create a new mruby String from `p[0..len]`.
    ///
    /// `len` is `mrb_int` which on wasm32 (MRB_INT32 config) is a 32-bit
    /// signed integer.
    pub fn mrb_str_new(mrb: *mut mrb_state, p: *const c_char, len: i32) -> mrb_value;

    /// `mrb_boxing_int_value(mrb, n)` — construct an mruby Integer value
    /// from a C `mrb_int`. Used to box integer RPC responses back into the
    /// mruby VM without string round-tripping.
    ///
    /// `n` is `mrb_int` which on wasm32 (MRB_INT32 config) is a 32-bit
    /// signed integer.
    pub fn mrb_boxing_int_value(mrb: *mut mrb_state, n: i32) -> mrb_value;

    /// `mrb_word_boxing_float_value(mrb, f)` — construct an mruby Float value
    /// via the word-boxing allocator. Used on wasm32 with
    /// MRB_WORDBOX_NO_INLINE_FLOAT where floats are heap-allocated.
    pub fn mrb_word_boxing_float_value(mrb: *mut mrb_state, f: f64) -> mrb_value;

    /// `mrb_define_method(mrb, c, name, func, aspec)` — defines an instance
    /// method on class `c`. Used to register instance-level `method_missing`
    /// on `Kobako::Handle` so handle objects forward method calls to the
    /// host via `Kobako.__rpc_call__`.
    pub fn mrb_define_method(
        mrb: *mut mrb_state,
        c: *mut RClass,
        name: *const c_char,
        func: mrb_func_t,
        aspec: mrb_aspec,
    );

    /// `mrb_obj_new(mrb, c, argc, argv)` — allocates and initializes a new
    /// instance of class `c`, calling `initialize` with `argc` arguments
    /// from `argv`. Used to create `Kobako::Handle` instances.
    pub fn mrb_obj_new(
        mrb: *mut mrb_state,
        c: *mut RClass,
        argc: i32,
        argv: *const mrb_value,
    ) -> mrb_value;

    /// `mrb_iv_set(mrb, obj, sym, val)` — sets the instance variable
    /// identified by `sym` on `obj` to `val`. Used by the Handle `initialize`
    /// C shim to stash the Handle id.
    pub fn mrb_iv_set(mrb: *mut mrb_state, obj: mrb_value, sym: mrb_sym, val: mrb_value);

    /// `mrb_iv_get(mrb, obj, sym)` — returns the instance variable identified
    /// by `sym` on `obj`, or `mrb_nil_value()` if not set.
    pub fn mrb_iv_get(mrb: *mut mrb_state, obj: mrb_value, sym: mrb_sym) -> mrb_value;

    /// `mrb_class_get(mrb, name)` — fetches a top-level class by name
    /// (e.g. `"RuntimeError"`). Used to resolve the parent class for
    /// `Kobako::ServiceError` / `Kobako::WireError` in
    /// `crate::kobako::Kobako::install_raw`.
    pub fn mrb_class_get(mrb: *mut mrb_state, name: *const c_char) -> *mut RClass;

    /// `mrb_module_get(mrb, name)` — fetches a top-level module by name
    /// (e.g. `"Kernel"`). Used to register `Kernel#puts` / `Kernel#p`
    /// via `mrb_define_method` without going through `mrb_load_nstring`.
    pub fn mrb_module_get(mrb: *mut mrb_state, name: *const c_char) -> *mut RClass;

    /// `mrb_ary_new_from_values(mrb, size, vals)` — constructs a new
    /// mruby Array containing `size` copies of the elements pointed to
    /// by `vals`. Used by `Kernel#p` to return the original args array
    /// when called with multiple arguments.
    pub fn mrb_ary_new_from_values(
        mrb: *mut mrb_state,
        size: i32,
        vals: *const mrb_value,
    ) -> mrb_value;

    /// `kobako_get_exc(mrb)` — layout-safe accessor for `mrb->exc`.
    ///
    /// Returns `mrb_obj_value(mrb->exc)` if an exception is pending, or
    /// `mrb_nil_value()` if `mrb->exc` is NULL. Implemented in
    /// `src/mrb_exc_helper.c` using mruby's own headers so that the
    /// struct field offset is always correct for the compiler and mruby
    /// version in use — no Rust-side byte-offset arithmetic required.
    ///
    /// Does NOT clear the exception. Callers must invoke `mrb_check_error`
    /// after consuming the returned value to reset `mrb->exc`.
    pub fn kobako_get_exc(mrb: *mut mrb_state) -> mrb_value;
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
