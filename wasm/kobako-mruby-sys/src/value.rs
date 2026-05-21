//! Typed `Value` newtype around the raw `mrb_value` FFI word-box.
//!
//! ## Why a newtype
//!
//! Three reasons stack here:
//!
//! 1. **Orphan rule** — `mrb_value` is declared at this crate's root
//!    so the FFI ABI stays accessible to other crates. Consumers
//!    (notably `kobako-wasm`) previously could not attach inherent
//!    methods to it; the predecessor of this module worked around
//!    that with a `MrbValueExt` extension trait. Wrapping the type
//!    inside this crate removes the trait + per-call-site `use`.
//! 2. **API surface clarity** — methods that operate on values
//!    (classname, to_string, predicates, unboxers) become inherent
//!    on `Value`, so the call shape is `val.classname(mrb)` rather
//!    than splatting raw FFI calls.
//! 3. **Migration anchor** — typed `Value` is the natural place to
//!    later attach typed variants (`MString`, `MArray`, `MHash`) and
//!    convert between them. Today no typed variants exist; the
//!    newtype is the floor on which they can be added.
//!
//! ## ABI guarantee
//!
//! `Value` is `#[repr(transparent)]` over [`mrb_value`]. The wasm32
//! `mrb_value` is a 4-byte word; `Value` is therefore also 4 bytes
//! and shares the C ABI. This matters at the `mrb_func_t` boundary:
//! a bridge declared with `Value` parameters and return type
//! produces the same wasm function signature as one declared with
//! `mrb_value`. Round-tripping through [`Value::from_raw`] /
//! [`Value::into_raw`] is therefore a no-op at the codegen level.
//!
//! ## What lives next to `Value` here
//!
//!   * The [`cstr!`] macro and [`cstr_ptr`] helper — generic
//!     NUL-terminated `*const c_char` plumbing; unchanged across
//!     the `Value` introduction.
//!   * The [`Immediates`] cache — `nil` / `true` / `false`
//!     `mrb_value` snapshots captured once via the layout-safe C
//!     shims, exposed through [`Value::nil`] / [`Value::true_`] /
//!     [`Value::false_`].

use crate as sys;

#[cfg(target_arch = "wasm32")]
use crate::Mrb;

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

// --------------------------------------------------------------------
// Immediates cache.
// --------------------------------------------------------------------
//
// `mrb_nil_value()` / `mrb_true_value()` / `mrb_false_value()` are
// config-level constants under mruby's word-box configuration — they
// are decided at libmruby build time and do not vary across
// `mrb_state` instances. Capturing them once via the C shims sidesteps
// a cross-FFI call every time a hot path wants `nil` / `true` /
// `false`. Previous home: `kobako::Immediates` in the consumer crate;
// re-located here so the cache ships from the same crate as `Value`.

#[cfg(target_arch = "wasm32")]
struct Immediates {
    qnil: sys::mrb_value,
    qtrue: sys::mrb_value,
    qfalse: sys::mrb_value,
}

// SAFETY: `mrb_value` on wasm32 is a `#[repr(C)] struct { w: u32 }` —
// plain old data with no interior mutability. `Immediates` therefore
// shares only `Copy` snapshots and is trivially Sync across the
// single-threaded wasm execution model.
#[cfg(target_arch = "wasm32")]
unsafe impl Sync for Immediates {}

#[cfg(target_arch = "wasm32")]
static IMMEDIATES: std::sync::OnceLock<Immediates> = std::sync::OnceLock::new();

#[cfg(target_arch = "wasm32")]
impl Immediates {
    /// Return the cached snapshot, capturing it on first call.
    fn get() -> &'static Immediates {
        IMMEDIATES.get_or_init(|| {
            // SAFETY: the three helpers are mruby's own
            // `mrb_nil_value` / `mrb_true_value` / `mrb_false_value`
            // (`MRB_INLINE`s reached through bindgen's static-fn
            // trampolines). They do not touch `mrb_state`.
            unsafe {
                Immediates {
                    qnil: sys::mrb_nil_value(),
                    qtrue: sys::mrb_true_value(),
                    qfalse: sys::mrb_false_value(),
                }
            }
        })
    }
}

// --------------------------------------------------------------------
// Value newtype.
// --------------------------------------------------------------------

/// Typed handle on a single mruby value. `#[repr(transparent)]` over
/// [`mrb_value`] so the C ABI is preserved.
///
/// Construct via [`Value::from_raw`] (at FFI boundaries),
/// [`Value::nil`] / [`Value::true_`] / [`Value::false_`] (immediates),
/// or [`Value::from_int`] / [`Value::from_float`] (numeric factories).
/// Round-trip back to the raw type via [`Value::as_raw`] /
/// [`Value::into_raw`] when calling raw FFI that has not yet been
/// migrated.
///
/// ## What is intentionally NOT here
///
/// No typed variants (`MString` / `MArray` / `MHash`). The
/// `mrb_value` word-box ABI is small enough that we keep passing
/// `Value` directly through the codebase. Typed variants can land
/// later as `pub struct MString(Value)` newtypes if the call sites
/// justify them.
///
/// ## Cross-target availability
///
/// `Value` itself, [`Value::from_raw`] / [`Value::as_raw`] /
/// [`Value::into_raw`] / [`Value::zeroed`], and the
/// [`sys::mrb_func_t`] typedef are available on every target so the
/// host-target signature-match tests
/// (`c_bridges_have_mrb_func_t_signature` in the consumer crate) keep
/// compiling. Methods that talk to mruby (`classname` / `call` /
/// numeric factories / predicates) live behind
/// `#[cfg(target_arch = "wasm32")]` because they would link against
/// unresolved mruby symbols on the host.
#[repr(transparent)]
#[derive(Copy, Clone)]
pub struct Value(pub(crate) sys::mrb_value);

impl Value {
    /// Wrap a raw `mrb_value` produced by FFI. The most common
    /// caller is a bridge function pointer receiving the receiver
    /// from mruby.
    #[inline]
    pub const fn from_raw(v: sys::mrb_value) -> Self {
        Self(v)
    }

    /// Borrow the inner `mrb_value` for raw FFI calls. Use this when
    /// passing the value through an as-yet-unmigrated `extern "C" fn`
    /// parameter. The wrapper itself stays usable after the borrow
    /// (`Value: Copy`).
    #[inline]
    pub const fn as_raw(self) -> sys::mrb_value {
        self.0
    }

    /// Consume and return the inner `mrb_value`. Identical to
    /// [`Value::as_raw`] semantically — `Value: Copy` makes the move
    /// vs. borrow distinction immaterial — but reads cleaner at the
    /// final return statement of a bridge function.
    #[inline]
    pub const fn into_raw(self) -> sys::mrb_value {
        self.0
    }

    /// All-zero `Value`. On wasm32 with the kobako mruby
    /// configuration this matches `mrb_nil_value()` (MRB_Qnil = 0),
    /// but callers that need a guaranteed nil should prefer
    /// [`Value::nil`] which reads through the mruby shim. The
    /// zeroed form exists for out-parameter initialization
    /// (`mrb_get_args` writes to it).
    #[inline]
    pub fn zeroed() -> Self {
        Self(sys::mrb_value::zeroed())
    }
}

#[cfg(target_arch = "wasm32")]
impl Value {
    /// Canonical mruby `nil`. Reads through the process-wide
    /// [`Immediates`] cache; capture is lazy and one-shot.
    #[inline]
    pub fn nil() -> Self {
        Self(Immediates::get().qnil)
    }

    /// Canonical mruby `true`. See [`Value::nil`].
    #[inline]
    pub fn true_() -> Self {
        Self(Immediates::get().qtrue)
    }

    /// Canonical mruby `false`. See [`Value::nil`].
    #[inline]
    pub fn false_() -> Self {
        Self(Immediates::get().qfalse)
    }

    /// `mrb_boxing_int_value(mrb, n)` — construct an mruby Integer
    /// from `n`. On wasm32 (`MRB_INT32`) the payload is signed 32-bit.
    #[inline]
    pub fn from_int(mrb: &Mrb, n: i32) -> Self {
        // SAFETY: `mrb` is alive by the `&Mrb` borrow.
        Self(unsafe { sys::mrb_boxing_int_value(mrb.as_ptr(), n) })
    }

    /// `mrb_word_boxing_float_value(mrb, f)` — construct an mruby
    /// Float from `f`. Used on wasm32 with
    /// `MRB_WORDBOX_NO_INLINE_FLOAT` where floats are heap-allocated.
    #[inline]
    pub fn from_float(mrb: &Mrb, f: f64) -> Self {
        // SAFETY: `mrb` is alive by the `&Mrb` borrow.
        Self(unsafe { sys::mrb_word_boxing_float_value(mrb.as_ptr(), f) })
    }

    /// `mrb_obj_classname(mrb, self)` — return the Ruby class name of
    /// `self` as a borrowed `&'static str`, or `""` when mruby
    /// returns NULL.
    ///
    /// The returned slice points into mruby's interned class-name
    /// storage, which lives for the duration of the `mrb_state`.
    /// Callers that need to retain the name across a GC point should
    /// `.to_string()` it.
    #[inline]
    pub fn classname(self, mrb: &Mrb) -> &'static str {
        // SAFETY: `mrb` is alive by the borrow; `self` originates
        // from the same VM by the single-VM contract.
        let ptr = unsafe { sys::mrb_obj_classname(mrb.as_ptr(), self.0) };
        if ptr.is_null() {
            return "";
        }
        // SAFETY: mruby's class-name storage lives for the duration
        // of the `mrb_state`; treating it as `'static` is sound for
        // the lifetime of the VM.
        unsafe { core::ffi::CStr::from_ptr(ptr) }
            .to_str()
            .unwrap_or("")
    }

    /// Coerce to a Rust `String` by calling `Object#to_s` and copying
    /// the bytes. `String#to_s` is idempotent on mruby Strings, so
    /// the redundant call is cheap and keeps a single conversion
    /// entry point.
    ///
    /// ## Exception handling
    ///
    /// If `.to_s` raises a Ruby exception (e.g. a user object
    /// overrides `to_s` with `raise`), the failure is **swallowed**:
    /// the pending `mrb->exc` is cleared via `mrb_check_error` and an
    /// empty `String` is returned. This prevents the leaked
    /// exception from corrupting subsequent mruby calls in the same
    /// C bridge.
    #[inline]
    pub fn to_string(self, mrb: &Mrb) -> String {
        let s_val = self.call(mrb, c"to_s", &[]);
        // SAFETY: `mrb` is alive by the borrow; `s_val` originates
        // from the same VM.
        let ptr = unsafe { sys::mrb_str_to_cstr(mrb.as_ptr(), s_val.0) };
        if ptr.is_null() {
            // `.to_s` raised or returned a non-String. Clear
            // `mrb->exc` so subsequent mruby calls in the same C
            // bridge don't see corrupted state.
            mrb.clear_exc();
            return String::new();
        }
        // SAFETY: mruby's `mrb_str_to_cstr` returns a NUL-terminated
        // pointer valid until the next GC cycle; copying the bytes
        // before any further mruby call is sound.
        unsafe { core::ffi::CStr::from_ptr(ptr) }
            .to_str()
            .unwrap_or("")
            .to_string()
    }

    /// Recover the `*mut RClass` pointer from a class-tagged
    /// `Value`. Implements the C macro `mrb_class_ptr(v)` —
    /// `((struct RClass*)(mrb_ptr(v)))` — inline so we do not have
    /// to declare it as an extern "C" fn (it is a macro, not a real
    /// C function).
    ///
    /// # Safety
    ///
    /// `self` must be a class-tagged `Value`.
    #[inline]
    pub unsafe fn as_class_ptr(self) -> *mut sys::RClass {
        self.0.w as *mut sys::RClass
    }

    /// Invoke `self.<method>(args...)` via the non-variadic
    /// `mrb_funcall_argv`. The method name is interned through
    /// [`Mrb::intern_cstr`].
    #[inline]
    pub fn call(self, mrb: &Mrb, name: &core::ffi::CStr, args: &[Value]) -> Value {
        let sym = mrb.intern_cstr(name);
        // Value is repr(transparent) over mrb_value, so &[Value] and
        // &[mrb_value] share layout. Cast the slice pointer.
        let argv = args.as_ptr() as *const sys::mrb_value;
        // SAFETY: `mrb` is alive by the borrow; `self` and every
        // `args` entry originate from the same VM by the single-VM
        // contract; `sym` was just interned against the same VM.
        Value(unsafe {
            sys::mrb_funcall_argv(
                mrb.as_ptr(),
                self.0,
                sym,
                args.len() as core::ffi::c_int,
                argv,
            )
        })
    }

    /// TRUE when `self` carries `MRB_TT_INTEGER`. Checks via mruby's
    /// own `mrb_type` (`MRB_INLINE`, reached through bindgen's
    /// static-fn trampoline). Pair with [`Value::unbox_integer`] for
    /// the direct-unbox path.
    #[inline]
    pub fn is_integer(self) -> bool {
        // SAFETY: mrb_type is a pure predicate over the value tag and
        // does not touch `mrb_state`.
        unsafe { sys::mrb_type(self.0) == sys::MRB_TT_INTEGER }
    }

    /// TRUE when `self` carries `MRB_TT_FLOAT`. See [`Value::is_integer`].
    /// Pair with [`Value::unbox_float`].
    #[inline]
    pub fn is_float(self) -> bool {
        // SAFETY: as `is_integer`.
        unsafe { sys::mrb_type(self.0) == sys::MRB_TT_FLOAT }
    }

    /// Direct `mrb_integer(v)` unbox via mruby's own
    /// `mrb_integer_func` helper (a `MRB_INLINE` reached through
    /// bindgen's static-fn trampoline).
    ///
    /// # Safety
    ///
    /// Caller must have confirmed Integer-tagging via
    /// [`Value::is_integer`]; calling on a non-Integer is undefined
    /// behaviour per mruby's macro contract.
    #[inline]
    pub unsafe fn unbox_integer(self) -> i32 {
        // SAFETY: forwarded from caller.
        unsafe { sys::mrb_integer_func(self.0) }
    }

    /// Direct `mrb_float(v)` unbox. Preserves full f64 precision.
    ///
    /// Under `MRB_WORDBOX_NO_INLINE_FLOAT` (the wasm32 kobako
    /// config) Float values are always object-tagged with two
    /// trailing zero bits — `mrb_value.w` is the `RFloat *` word
    /// directly, so we cast through it instead of routing through
    /// the `mrb_val_union` static inline (whose `union mrb_value_`
    /// return type carries a wasm32 FFI ABI mismatch between
    /// bindgen's trampoline and rustc). The result is fed to
    /// mruby's own `mrb_rfloat_value` (`static inline`, reached via
    /// bindgen's static-fn trampoline) which reads the f64 payload
    /// out of `RFloat`.
    ///
    /// # Safety
    ///
    /// As [`Value::unbox_integer`]: caller has confirmed Float-tagging.
    #[inline]
    pub unsafe fn unbox_float(self) -> f64 {
        // SAFETY: forwarded from caller. Float-tagged values have
        // the object-tag bit pattern, so `w` aliases an `RFloat *`.
        let fp = self.0.w as *mut sys::RFloat;
        unsafe { sys::mrb_rfloat_value(fp) }
    }

    /// `mrb_ary_entry(self, idx)` — read the element at `idx` from
    /// `self` (which must be an Array `Value`). No bounds checking;
    /// caller must keep `idx` within `0..self.length`.
    ///
    /// # Safety
    ///
    /// `self` must be an Array-tagged `Value`. Out-of-range `idx`
    /// returns `mrb_nil_value` rather than reading past the buffer;
    /// passing a non-Array yields an undefined `Value`.
    #[inline]
    pub unsafe fn ary_entry(self, idx: i32) -> Value {
        // SAFETY: forwarded from caller.
        Value(unsafe { sys::mrb_ary_entry(self.0, idx) })
    }

    // ----------------------------------------------------------------
    // Instance variable / constant accessors. The mruby C API spells
    // these as `mrb_iv_set` / `mrb_iv_get` / `mrb_const_defined` /
    // `mrb_const_get` / `mrb_respond_to`; the inherent methods carry
    // the same names so the call shape mirrors the C-side
    // documentation one-to-one. The `&Mrb` borrow upholds liveness,
    // and `self` provides the receiver — together the methods are
    // safe Rust.
    // ----------------------------------------------------------------

    /// `mrb_iv_set(mrb, self, sym, val)` — assign instance variable
    /// `sym` on `self` to `val`. `self` must be an object value
    /// produced by `mrb`.
    #[inline]
    pub fn iv_set(self, mrb: &Mrb, sym: sys::mrb_sym, val: Value) {
        // SAFETY: `mrb` is alive by the borrow; `self` and `val`
        // originate from the same VM by the single-VM contract.
        unsafe { sys::mrb_iv_set(mrb.as_ptr(), self.0, sym, val.0) };
    }

    /// `mrb_iv_get(mrb, self, sym)` — return instance variable `sym`
    /// from `self`, or `nil` when unset.
    #[inline]
    pub fn iv_get(self, mrb: &Mrb, sym: sys::mrb_sym) -> Value {
        // SAFETY: as `iv_set`.
        Value(unsafe { sys::mrb_iv_get(mrb.as_ptr(), self.0, sym) })
    }

    /// `mrb_const_defined(mrb, self, sym)` — TRUE when constant `sym`
    /// is defined on `self` (the module or class value).
    #[inline]
    pub fn const_defined(self, mrb: &Mrb, sym: sys::mrb_sym) -> bool {
        // SAFETY: as `iv_set`.
        unsafe { sys::mrb_const_defined(mrb.as_ptr(), self.0, sym) }
    }

    /// `mrb_const_get(mrb, self, sym)` — fetch the constant value at
    /// `sym` from `self`. Sets `mrb->exc` if the constant is
    /// undefined; callers should gate with [`Value::const_defined`].
    #[inline]
    pub fn const_get(self, mrb: &Mrb, sym: sys::mrb_sym) -> Value {
        // SAFETY: as `iv_set`.
        Value(unsafe { sys::mrb_const_get(mrb.as_ptr(), self.0, sym) })
    }

    /// `mrb_respond_to(mrb, self, mid)` — TRUE when `self` answers to
    /// the method named by `mid`.
    #[inline]
    pub fn respond_to(self, mrb: &Mrb, mid: sys::mrb_sym) -> bool {
        // SAFETY: as `iv_set`.
        unsafe { sys::mrb_respond_to(mrb.as_ptr(), self.0, mid) }
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
