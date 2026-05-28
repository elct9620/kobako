//! Top-level module / class registration and global-state mutation
//! on `Mrb`.
//!
//! Inherent methods that register names against the Object root or
//! the global variable table:
//!
//!   * `mrb_define_module` / `mrb_define_class` — register a new
//!     module or class at top level.
//!   * `mrb_class_get` — look one up by name.
//!   * `mrb_define_global_const` — bind a top-level constant.
//!   * `mrb_gv_set` — assign a Ruby `$global`.
//!
//! Nested-namespace counterparts (`mrb_define_module_under`,
//! `mrb_define_class_under`, `mrb_class_get_under`) live on
//! `crate::Class` because they take an outer class/module receiver,
//! not the VM root.

#[cfg(target_arch = "wasm32")]
use crate::{Class, Mrb, Value};
#[cfg(target_arch = "wasm32")]
use mruby_sys as sys;

#[cfg(target_arch = "wasm32")]
impl Mrb {
    /// `mrb_define_module(mrb, name)` — return the module named
    /// `name`, defining it at top level if not already present.
    #[inline]
    pub fn define_module(&self, name: &core::ffi::CStr) -> Class {
        // SAFETY: `self` is alive; `name` is NUL-terminated.
        Class::from_raw(unsafe { sys::mrb_define_module(self.as_ptr(), name.as_ptr()) })
    }

    /// `mrb_define_class(mrb, name, super_)` — define a top-level
    /// class named `name` inheriting from `super_`.
    #[inline]
    pub fn define_class(&self, name: &core::ffi::CStr, super_: Class) -> Class {
        // SAFETY: `self` is alive; `name` is NUL-terminated; `super_`
        // was produced by the same VM.
        Class::from_raw(unsafe {
            sys::mrb_define_class(self.as_ptr(), name.as_ptr(), super_.as_raw())
        })
    }

    /// `mrb_class_get(mrb, name)` — fetch the top-level class named
    /// `name`. The returned `Class` may be null when no such class
    /// is registered.
    #[inline]
    pub fn class_get(&self, name: &core::ffi::CStr) -> Class {
        // SAFETY: `self` is alive; `name` is NUL-terminated.
        Class::from_raw(unsafe { sys::mrb_class_get(self.as_ptr(), name.as_ptr()) })
    }

    /// `mrb_define_global_const(mrb, name, val)` — bind a top-level
    /// constant. Reachable as `name` and as `Object::name`.
    #[inline]
    pub fn define_global_const(&self, name: &core::ffi::CStr, val: Value) {
        // SAFETY: `self` is alive; `name` is NUL-terminated; `val`
        // originates from the same VM.
        unsafe { sys::mrb_define_global_const(self.as_ptr(), name.as_ptr(), val.as_raw()) };
    }

    /// `mrb_gv_set(mrb, sym, val)` — assign a global variable.
    #[inline]
    pub fn gv_set(&self, sym: sys::mrb_sym, val: Value) {
        // SAFETY: `self` is alive; `val` originates from the same VM.
        unsafe { sys::mrb_gv_set(self.as_ptr(), sym, val.as_raw()) };
    }
}
