//! mruby — typed Rust wrapper over the `mruby-sys` FFI surface.
//!
//! This crate is the planned home of every Rust-level abstraction
//! above the mruby C API: the `Mrb` / `Ccontext` RAII types, the
//! `Value` / `Class` / `Array` / `Hash` newtypes, the `IntoValue` /
//! `FromValue` trait seam, the `Format`-based `mrb_get_args`
//! dispatch, and the `protect` closure wrapper. `mruby-sys` keeps
//! only the bindgen-generated `extern "C"` declarations and the
//! layout-safe C shims — the same split magnus + rb-sys land on for
//! CRuby.
//!
//! ## Transparent proxy (current state)
//!
//! Today this crate is a single-line proxy: every public name in
//! `mruby-sys` is re-exported here unchanged, and `mruby::sys`
//! aliases the entire `mruby-sys` crate for the raw-FFI escape
//! hatch. Consumers can already switch their imports from
//! `use mruby_sys::Mrb` to `use mruby::Mrb` against this scaffold;
//! the physical move of the typed wrappers from `mruby-sys/src/` to
//! `mruby/src/` is a follow-up step that turns the proxy into the
//! real wrapper without touching the consumer surface.

#![allow(non_camel_case_types)]
#![allow(non_upper_case_globals)]
#![allow(non_snake_case)]

// Wholesale re-export of the `mruby-sys` surface. Once the typed
// wrappers physically move into this crate, this glob shrinks to
// just the raw FFI names that genuinely belong to the sys layer
// (`mrb_value`, `mrb_state`, `mrb_func_t`, the `mrb_args_*`
// helpers, …); the typed names will then be owned modules here.
#[cfg(any(target_arch = "wasm32", test))]
pub use mruby_sys::*;

/// Raw FFI escape hatch. Use `mruby::sys::mrb_*` when the typed API
/// in this crate's root does not yet cover a needed symbol. Anything
/// promoted out of this namespace into the typed surface should
/// disappear from new call sites over time.
pub use mruby_sys as sys;
