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
}
