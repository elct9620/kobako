//! Façade re-exporting the mruby C-API binding from the sibling
//! `kobako-mruby-sys` crate.
//!
//! Existing call sites continue to spell their imports as
//! `use crate::mruby::sys;` / `use crate::mruby::Mrb;` /
//! `use crate::mruby::Ccontext;` — this module forwards each to its
//! real home in `kobako-mruby-sys`. The submodules that previously
//! lived here (`state.rs`, `ccontext.rs`, `value.rs`) have moved into
//! `kobako-mruby-sys/src/` alongside the FFI declarations they wrap.
//!
//! This façade exists so the typed-newtype migration could land
//! incrementally without touching every `use crate::mruby::*` in the
//! codebase. Now that `Value` / `Class` are in place, the next step
//! is to retire the façade — either by switching every call site to
//! `use kobako_mruby_sys as sys;` / `use kobako_mruby_sys::Mrb;`
//! directly, or by collapsing this module into a single
//! `pub use kobako_mruby_sys;` line. Left in place for now so this
//! commit stays a pure clean-up of stale references.

pub use kobako_mruby_sys as sys;

#[cfg(target_arch = "wasm32")]
pub use kobako_mruby_sys::Mrb;

// `Value` is reached via `crate::mruby::sys::Value` at call sites;
// no shorter re-export here to avoid an unused-import warning while
// keeping the canonical path explicit. Once Class arrives the same
// principle applies (`crate::mruby::sys::Class`).

#[cfg(target_arch = "wasm32")]
pub use kobako_mruby_sys::Ccontext;

#[cfg(target_arch = "wasm32")]
pub use kobako_mruby_sys::cstr_ptr;

// Re-export the `cstr!` macro at the consumer crate's root so the
// existing `use crate::cstr;` pattern at every FFI call site keeps
// resolving. The macro itself ships from `kobako-mruby-sys` with
// `#[macro_export]`; this re-export lives in `lib.rs`.
