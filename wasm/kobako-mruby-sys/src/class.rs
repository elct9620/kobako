//! Typed `Class` newtype around `*mut RClass`.
//!
//! ## Why a newtype
//!
//! Same rationale as [`Value`](crate::Value): the raw pointer crosses
//! the crate boundary, and consumers historically had to pass it
//! around as `*mut sys::RClass` — easy to leak, easy to confuse with
//! other opaque pointers, and impossible to attach inherent methods
//! to from a sibling crate without an extension trait. Wrapping the
//! pointer in this crate gives every consumer a typed handle plus a
//! single place to grow method surface (class lookup, method
//! definition, class-name resolution).
//!
//! ## ABI guarantee
//!
//! `Class` is `#[repr(transparent)]` over `*mut RClass`. The wasm32
//! pointer width is 4 bytes; `Class` is therefore also 4 bytes and
//! shares the C ABI. This matters at the FFI boundary — a struct
//! field of type `Class` round-trips into mruby's own `RClass *`
//! slot without any conversion.
//!
//! ## Module alias
//!
//! mruby represents both Modules and Classes with the same
//! `struct RClass` type at the C level. `Module` is a transparent
//! type alias for `Class` so install paths can read
//! `let kobako_mod: Module = …` for a module handle and
//! `let handle_class: Class = …` for a class handle without changing
//! the underlying representation.
//!
//! ## Null representation
//!
//! `Class` may carry a null pointer because several mruby APIs
//! (`mrb_class_get_under`, `mrb_class_get`) signal "not found" by
//! returning NULL. Consumers gate on [`Class::is_null`] before
//! treating the value as a live class handle; a future typed-error
//! migration could move null handling into the return type.

#[cfg(target_arch = "wasm32")]
use crate as sys;
#[cfg(target_arch = "wasm32")]
use crate::Mrb;
#[cfg(target_arch = "wasm32")]
use crate::Value;

/// Typed handle on an mruby class / module. `#[repr(transparent)]`
/// over `*mut RClass` so the C ABI is preserved.
///
/// Construct via [`Class::from_raw`] at FFI boundaries. Round-trip
/// back to the raw pointer via [`Class::as_raw`] when calling raw
/// mruby APIs (`mrb_define_method`, `mrb_define_class_under`, …).
///
/// Available on both targets so the consumer-side
/// [`crate::mrb_func_t`] signature-match tests keep compiling on
/// host. Methods that talk to mruby live behind
/// `#[cfg(target_arch = "wasm32")]`.
#[repr(transparent)]
#[derive(Copy, Clone)]
pub struct Class(pub(crate) *mut crate::RClass);

/// Alias for [`Class`] used by install paths to express "this handle
/// refers to a module" without changing the runtime type.
pub type Module = Class;

impl Class {
    /// Wrap a raw `*mut RClass` produced by FFI. Most call sites get
    /// the pointer from `mrb_define_class_under`,
    /// `mrb_class_get_under`, `mrb_class_get`, or
    /// [`crate::mrb_object_class`].
    #[inline]
    pub const fn from_raw(p: *mut crate::RClass) -> Self {
        Self(p)
    }

    /// Borrow the inner `*mut RClass` for raw FFI calls. The wrapper
    /// itself stays usable after the borrow (`Class: Copy`).
    #[inline]
    pub const fn as_raw(self) -> *mut crate::RClass {
        self.0
    }

    /// TRUE when the underlying pointer is null. The mruby class
    /// lookup APIs (`mrb_class_get_under`, `mrb_class_get`) return
    /// NULL for "not found"; consumers can gate on this before
    /// dereferencing.
    #[inline]
    pub fn is_null(self) -> bool {
        self.0.is_null()
    }

    /// Reify this class handle as an mruby [`Value`] via the
    /// layout-safe `kobako_class_value` C shim
    /// (wrapper around mruby's `mrb_obj_value(p)` inline). Used by
    /// call paths that need to pass the class through generic mruby
    /// APIs that accept `mrb_value` (e.g. `mrb_const_defined` /
    /// `mrb_const_get` / `Object#constants`).
    ///
    /// # Safety
    ///
    /// `self` must be a live class handle produced by the same VM
    /// as `mrb` (and not yet freed).
    #[cfg(target_arch = "wasm32")]
    #[inline]
    pub unsafe fn as_value(self, _mrb: &Mrb) -> Value {
        // SAFETY: forwarded from caller; the shim reads only the
        // pointer payload and reuses mruby's own boxing logic.
        Value::from_raw(unsafe { sys::kobako_class_value(self.0) })
    }

    // ----------------------------------------------------------------
    // Nested module / class registration, method definition,
    // instantiation, and raise. Each method mirrors the mruby C API:
    //
    //   - mrb_define_module_under(mrb, outer, name)
    //   - mrb_define_class_under(mrb, outer, name, super_)
    //   - mrb_class_get_under(mrb, outer, name)
    //   - mrb_class_name(mrb, c)
    //   - mrb_define_method(mrb, c, name, func, aspec)
    //   - mrb_define_singleton_method(mrb, obj, name, func, aspec)
    //   - mrb_obj_new(mrb, c, argc, argv)
    //   - mrb_raise(mrb, c, msg)  [diverges]
    //
    // `Module` is a type alias for `Class`, so the `module_under` /
    // `class_under` pair is reachable from either spelling without
    // changing the runtime type.
    // ----------------------------------------------------------------

    /// `mrb_define_module_under(mrb, self, name)` — define or fetch a
    /// nested module under `self`.
    #[cfg(target_arch = "wasm32")]
    #[inline]
    pub fn define_module_under(self, mrb: &Mrb, name: &core::ffi::CStr) -> Class {
        // SAFETY: `mrb` is alive; `self` was produced by the same VM;
        // `name` is NUL-terminated.
        Class::from_raw(unsafe {
            sys::mrb_define_module_under(mrb.as_ptr(), self.0, name.as_ptr())
        })
    }

    /// `mrb_define_class_under(mrb, self, name, super_)` — define a
    /// nested class under `self`, inheriting from `super_`.
    #[cfg(target_arch = "wasm32")]
    #[inline]
    pub fn define_class_under(self, mrb: &Mrb, name: &core::ffi::CStr, super_: Class) -> Class {
        // SAFETY: as `define_module_under`; `super_` originates from
        // the same VM.
        Class::from_raw(unsafe {
            sys::mrb_define_class_under(mrb.as_ptr(), self.0, name.as_ptr(), super_.0)
        })
    }

    /// `mrb_class_get_under(mrb, self, name)` — fetch a nested class
    /// by name. The returned [`Class`] may be null when no such class
    /// is registered.
    #[cfg(target_arch = "wasm32")]
    #[inline]
    pub fn class_get_under(self, mrb: &Mrb, name: &core::ffi::CStr) -> Class {
        // SAFETY: as `define_module_under`.
        Class::from_raw(unsafe { sys::mrb_class_get_under(mrb.as_ptr(), self.0, name.as_ptr()) })
    }

    /// `mrb_class_name(mrb, self)` — return the class's full Ruby
    /// name (e.g. `"MyService::KV"`). Returns `None` when mruby
    /// yields NULL.
    ///
    /// The returned slice points into mruby's interned class-name
    /// storage which lives for the duration of the VM.
    #[cfg(target_arch = "wasm32")]
    #[inline]
    pub fn name(self, mrb: &Mrb) -> Option<&'static str> {
        // SAFETY: `mrb` is alive by the borrow; `self` originates
        // from the same VM by the single-VM contract.
        let ptr = unsafe { sys::mrb_class_name(mrb.as_ptr(), self.0) };
        if ptr.is_null() {
            return None;
        }
        // SAFETY: mruby's class-name storage lives for the duration
        // of the VM.
        Some(
            unsafe { core::ffi::CStr::from_ptr(ptr) }
                .to_str()
                .unwrap_or(""),
        )
    }

    /// `mrb_define_method(mrb, self, name, func, aspec)` — register
    /// an instance method on this class.
    #[cfg(target_arch = "wasm32")]
    #[inline]
    pub fn define_method(
        self,
        mrb: &Mrb,
        name: &core::ffi::CStr,
        func: sys::mrb_func_t,
        aspec: sys::mrb_aspec,
    ) {
        // SAFETY: `mrb` is alive; `self` was produced by the same
        // VM; `name` is NUL-terminated; `func` matches the
        // `mrb_func_t` ABI by type.
        unsafe { sys::mrb_define_method(mrb.as_ptr(), self.0, name.as_ptr(), func, aspec) };
    }

    /// `mrb_define_singleton_method(mrb, self, name, func, aspec)` —
    /// register a singleton-class method on this class object. The
    /// receiver here is treated as `RObject *` so the singleton-class
    /// shim attaches to the metaclass (matching mruby's own contract).
    #[cfg(target_arch = "wasm32")]
    #[inline]
    pub fn define_singleton_method(
        self,
        mrb: &Mrb,
        name: &core::ffi::CStr,
        func: sys::mrb_func_t,
        aspec: sys::mrb_aspec,
    ) {
        // SAFETY: as `define_method`. `RClass *` and `RObject *` are
        // both `c_void *` aliases in this crate's binding; the cast
        // matches what `mrbgems/mruby-singleton-class` does inline.
        unsafe {
            sys::mrb_define_singleton_method(
                mrb.as_ptr(),
                self.0 as *mut sys::RObject,
                name.as_ptr(),
                func,
                aspec,
            )
        };
    }

    /// `mrb_obj_new(mrb, self, argc, argv)` — allocate and initialise
    /// a new instance of this class, calling `initialize` with `args`.
    #[cfg(target_arch = "wasm32")]
    #[inline]
    pub fn obj_new(self, mrb: &Mrb, args: &[Value]) -> Value {
        // Value is repr(transparent) over mrb_value; the slice
        // pointer reuses the same layout.
        let argv = args.as_ptr() as *const sys::mrb_value;
        // SAFETY: `mrb` is alive; `self` and every `args` entry
        // originate from the same VM.
        Value::from_raw(unsafe { sys::mrb_obj_new(mrb.as_ptr(), self.0, args.len() as i32, argv) })
    }

    /// `mrb_raise(mrb, self, msg)` — raise an exception of this class
    /// with `msg`. Diverges — `mrb_raise` long-jumps out and never
    /// returns to the caller.
    ///
    /// # Safety
    ///
    /// Only callable from contexts that mruby may unwind out of (C
    /// bridges, `mrb_funcall` handlers, `mrb_protect_error` bodies).
    /// Calling from arbitrary Rust code would skip Rust drop frames
    /// the stack expects to run.
    #[cfg(target_arch = "wasm32")]
    #[inline]
    pub unsafe fn raise(self, mrb: &Mrb, msg: &core::ffi::CStr) -> ! {
        // SAFETY: bridge frame — caller upholds the unwind contract.
        // bindgen drops the `mrb_noreturn` attribute on its `mrb_raise`
        // declaration, so the FFI return type is `()` rather than the
        // diverging `!`. The `unreachable_unchecked` keeps the
        // diverging Rust signature without an extra runtime branch —
        // `mrb_raise` long-jumps before control can reach it.
        unsafe { sys::mrb_raise(mrb.as_ptr(), self.0, msg.as_ptr()) };
        unsafe { core::hint::unreachable_unchecked() }
    }
}
