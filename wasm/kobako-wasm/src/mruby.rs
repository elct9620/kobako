//! Mruby C-API binding surface.
//!
//! Two submodules:
//!
//!   * [`sys`] — hand-rolled `extern "C"` declarations for the mruby C
//!     API subset the Guest Binary calls. All FFI symbol-level work lives
//!     here; see the module docs for the bindgen trade-off.
//!   * [`value`] — small ergonomic layer over `sys::mrb_value` (inherent
//!     methods + the [`cstr!`] macro re-export). Designed to mirror the
//!     `magnus::Value` shape for CRuby — value-centric methods on the
//!     value type, byte-string utilities as free items.
//!
//! Subsequent items will add a `state` submodule introducing an
//! `Mrb`/`MrbRef` newtype pair that owns/borrows `*mut sys::mrb_state`
//! and concentrates the remaining `unsafe` blocks behind safe methods.

pub mod state;
pub mod sys;
pub mod value;

pub use state::{Mrb, MrbOpenError};
