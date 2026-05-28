//! Façade re-exporting the typed mruby surface from the sibling
//! `mruby` crate so call sites can spell their imports as
//! `use crate::mruby::sys;` / `use crate::mruby::Mrb;` /
//! `use crate::mruby::{Value, Class, …}` against this crate's
//! module tree instead of cracking back out to the `mruby` crate
//! root at every site.
//!
//! The single glob re-export below covers every pub item at the
//! `mruby` crate root — typed wrappers (`Mrb`, `Ccontext`, `Value`,
//! `Class`, `Array`, `Hash`), the `IntoValue` / `FromValue` /
//! `Format` traits, the `format` module of ZST markers, the typed
//! `mrb_func_t` alias, and the `sys` re-export of `mruby-sys` for
//! the raw-FFI escape hatch (`crate::mruby::sys::mrb_value` etc.).

#[cfg(target_arch = "wasm32")]
pub use mruby::*;
