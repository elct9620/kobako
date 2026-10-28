//! The SDK's integration tests, built as one binary so a run links the
//! SDK and its engine once rather than once per file.

// Only the cases behind the `wasmtime` feature load a Guest Binary; the
// others stand in any build.
#[cfg(feature = "wasmtime")]
mod common;

mod byte_surface;
mod concurrency;
mod fillable;
mod flags;
mod install;
mod mount;
mod overrides;
mod runtime_injection;
mod snippet_compile;
mod value_budget;
