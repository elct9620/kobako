//! The driver's integration tests, built as one binary so a run links
//! wasmtime once rather than once per file.

mod support;

mod compile_posture;
mod guest_file;
mod preamble_flags;
mod snippet_compile;
mod unanswered_call;
mod unsupported_call;
