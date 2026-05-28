//! mruby — typed Rust wrapper over the `mruby-sys` FFI surface.
//!
//! This crate owns every Rust-level abstraction above the mruby C
//! API: the `Mrb` / `Ccontext` RAII types, the `Value` / `Class` /
//! `Array` / `Hash` newtypes, the `IntoValue` / `FromValue` trait
//! seam, the `Format`-based `mrb_get_args` dispatch, and the
//! `protect` closure wrapper. The sibling `mruby-sys` crate keeps
//! only the bindgen-generated `extern "C"` declarations and the
//! layout-safe C shims — the same split magnus + rb-sys apply at
//! the CRuby boundary.
//!
//! ## Layering
//!
//! ```text
//! L2  trait seams      value::convert (IntoValue / FromValue)
//!                      state::args    (Format trait + ZST + GAT dispatch)
//!                      state::protect (closure-based mrb_protect_error)
//!
//! L1  RAII / newtypes  state         (Mrb owning *mut mrb_state)
//!                      value         (Value newtype + cstr! / cstr_ptr)
//!                      class         (Class / Module handles)
//!                      array / hash  (typed factories on top of Value)
//!                      ccontext      (Ccontext RAII)
//!
//! L0  raw FFI          mruby-sys::*  (bindgen output + ABI constants)
//! ```
//!
//! ## Raw-FFI escape hatch
//!
//! `mruby::sys` re-exports the entire `mruby-sys` crate so call
//! sites that still need the raw bindgen surface
//! (`sys::mrb_value`, `sys::mrb_state`, `sys::mrb_func_t`,
//! `sys::mrb_args_*`, …) keep a short import path. Anything that
//! becomes wrappable in the typed surface above should leave this
//! escape hatch over time.

#![allow(non_camel_case_types)]
#![allow(non_upper_case_globals)]
#![allow(non_snake_case)]

// Safe-layer modules. These hold the kobako abstractions over the
// bindgen FFI surface: `Mrb` / `Ccontext` RAII, typed `Value` /
// `Class` / `Array` / `Hash` newtypes, and the `cstr!` / `cstr_ptr`
// C-string helpers.
//
// `ccontext` / `array` / `hash` / `convert` are wasm32-only because
// their bodies call mruby functions that are only linked on wasm32;
// including them on host targets would surface `unresolved import`
// errors as soon as `cargo test` ran the crate on the host target.
#[cfg(target_arch = "wasm32")]
pub mod array;
#[cfg(target_arch = "wasm32")]
pub mod ccontext;
pub mod class;
#[cfg(target_arch = "wasm32")]
pub mod convert;
#[cfg(target_arch = "wasm32")]
pub mod hash;
pub mod state;
pub mod value;

#[cfg(target_arch = "wasm32")]
pub use state::{Mrb, MrbOpenError};

#[cfg(target_arch = "wasm32")]
pub use state::args::{format, Format};

#[cfg(target_arch = "wasm32")]
pub use ccontext::Ccontext;

#[cfg(target_arch = "wasm32")]
pub use array::Array;
pub use class::{Class, Module};
#[cfg(target_arch = "wasm32")]
pub use convert::{FromValue, IntoValue};
#[cfg(target_arch = "wasm32")]
pub use hash::Hash;
pub use value::cstr_ptr;
pub use value::Value;

/// Raw FFI escape hatch. Use `mruby::sys::mrb_*` when the typed API
/// in this crate's root does not yet cover a needed symbol. Anything
/// promoted out of this namespace into the typed surface should
/// disappear from new call sites over time.
pub use mruby_sys as sys;

/// Typed counterpart of `sys::mrb_func_t` using the `Value` newtype
/// for the receiver and return slots. `Value` is
/// `#[repr(transparent)]` over `mrb_value`, so this alias has the
/// same C ABI as `sys::mrb_func_t` — but Rust nominal typing keeps
/// the two distinct, which lets `Class::define_method` accept
/// bridges declared with the ergonomic typed signature without an
/// `as`-cast at every call site. The `transmute` from this typed
/// alias to `sys::mrb_func_t` happens once inside
/// `Class::define_method` / `define_singleton_method`.
#[cfg(target_arch = "wasm32")]
pub type mrb_func_t = unsafe extern "C" fn(mrb: *mut sys::mrb_state, self_: Value) -> Value;
