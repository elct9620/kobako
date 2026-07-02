//! Per-`Runtime` execution configuration.
//!
//! The wall-clock and per-channel capture caps a `Kobako::Sandbox`
//! forwards into `Runtime::from_path`. A plain value carrier owned by the
//! `Runtime` — distinct from the process-wide engine/module `super::cache`
//! (which is shared across every Sandbox) and from the per-invocation
//! `super::invocation::Invocation` (which the wasm engine mutates from
//! inside a run). These caps are read only by `Runtime` methods between
//! runs, so they live here.

use std::time::Duration;

/// Wall-clock and output caps for one `Runtime`. `None` on any field
/// disables that cap.
pub(super) struct Config {
    /// Wall-clock cap for one guest `#eval` / `#run`. Stamped into a
    /// per-run `Instant` deadline by `Driver::prime_caps`.
    pub(super) timeout: Option<Duration>,
    /// Byte cap for guest stdout capture.
    /// Sizes the per-run `MemoryOutputPipe` and computes the truncation
    /// flag in `Driver::build_snapshot`.
    pub(super) stdout_limit_bytes: Option<usize>,
    /// Byte cap for guest stderr capture. Mirror of `stdout_limit_bytes`.
    pub(super) stderr_limit_bytes: Option<usize>,
}
