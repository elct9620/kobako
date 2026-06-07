//! Façade re-exporting the typed mruby surface from the `beni` crate
//! so call sites can spell their imports as
//! `use crate::mruby::sys;` / `use crate::mruby::Mrb;` /
//! `use crate::mruby::{Value, RClass, …}` against this crate's
//! module tree instead of cracking back out to the `beni` crate
//! root at every site.
//!
//! The single glob re-export below covers every pub item at the
//! `beni` crate root — typed wrappers (`Mrb`, `Ccontext`, `Value`,
//! `RClass`, `RModule`, `Array`, `Hash`), the `IntoValue` /
//! `FromValue` / `Format` / `Module` / `Object` traits, the `format`
//! module of ZST markers, the typed `mrb_func_t` alias, `MethodDef`,
//! `Error`, and the `sys` re-export of `beni-sys` for the raw-FFI
//! escape hatch (`crate::mruby::sys::mrb_value` etc.).

#[cfg(target_arch = "wasm32")]
pub use beni::*;
