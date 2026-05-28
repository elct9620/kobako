//! Façade re-exporting the typed mruby surface from the sibling
//! `mruby` crate.
//!
//! Existing call sites continue to spell their imports as
//! `use crate::mruby::sys;` / `use crate::mruby::Mrb;` /
//! `use crate::mruby::Ccontext;` — this module forwards each to its
//! real home in the `mruby` crate, which in turn re-exports
//! `mruby-sys` through its `sys` namespace for raw-FFI access.
//!
//! This façade is the migration anchor for typed-newtype adoption.
//! Now that every consumer path resolves through `mruby::*`, the
//! next step is to retire the façade — either by switching every
//! call site to `use mruby as sys;` / `use mruby::Mrb;` directly,
//! or by collapsing this module into a single `pub use mruby;` line.
//! Left in place for now so this commit stays a pure import switch.

#[cfg(target_arch = "wasm32")]
pub use mruby::sys;

#[cfg(target_arch = "wasm32")]
pub use mruby::{format, Array, Ccontext, Class, FromValue, Hash, IntoValue, Mrb, Value};

// Re-export the `cstr!` macro at the consumer crate's root so the
// existing `use crate::cstr;` pattern at the few remaining raw-FFI
// call sites (e.g. `mrb_get_args` format strings) keeps resolving.
// `#[macro_export]` exports a macro from its defining crate's root,
// so re-anchoring it here is the minimum-diff bridge. The macro
// itself ships from `mruby` (the typed wrapper crate, now that
// `value.rs` has moved); the re-export lives in `lib.rs`.
