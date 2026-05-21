//! Mruby C-API binding surface.
//!
//! Three pieces stack here:
//!
//!   * [`sys`] — re-exported from the sibling `kobako-mruby-sys` crate.
//!     Carries the hand-rolled `extern "C"` declarations plus the four
//!     layout-safe C shims compiled by that crate's `build.rs`. All
//!     symbol-level FFI work lives there; the re-export keeps every
//!     `use crate::mruby::sys;` call site intact after the sys split.
//!   * [`value`] — small ergonomic layer over `sys::mrb_value` (inherent
//!     methods + the [`cstr!`] macro re-export). Designed to mirror the
//!     `magnus::Value` shape for CRuby — value-centric methods on the
//!     value type, byte-string utilities as free items.
//!   * [`state`] — `Mrb` RAII wrapper around `mrb_state *` (open / close
//!     lifecycle plus the pending-exception accessors).
//!   * [`ccontext`] — `Ccontext` RAII wrapper around `mrb_ccontext *`
//!     (compile-context allocation, filename stamping, and
//!     `mrb_load_nstring_cxt` invocation).

#[cfg(target_arch = "wasm32")]
pub mod ccontext;
pub mod state;
pub mod value;

pub use kobako_mruby_sys as sys;

#[cfg(target_arch = "wasm32")]
pub use state::Mrb;

#[cfg(target_arch = "wasm32")]
pub use value::MrbValueExt;
