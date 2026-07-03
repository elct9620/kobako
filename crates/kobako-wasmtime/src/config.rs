//! Per-`Driver` execution configuration.
//!
//! The wall-clock and per-channel capture caps a frontend forwards into
//! `Driver::new`. A plain value carrier owned by the `Driver` — distinct
//! from the process-wide engine/module `crate::cache` (which is shared
//! across every sandbox) and from the per-invocation
//! `crate::invocation::Invocation` (which the wasm engine mutates from
//! inside a run). These caps are read only by `Driver` methods between
//! runs, so they live here.

use std::time::Duration;

use kobako_runtime::profile::Profile;

/// Wall-clock and output caps plus the requested isolation profile for
/// one `Driver`. `None` on any cap field disables that cap.
pub struct Config {
    /// Wall-clock cap for one guest `#eval` / `#run`. Stamped into a
    /// per-run `Instant` deadline by `Driver::prime_caps`.
    pub timeout: Option<Duration>,
    /// Byte cap for guest stdout capture.
    /// Sizes the per-run `MemoryOutputPipe` and computes the truncation
    /// flag in `Driver::build_snapshot`.
    pub stdout_limit_bytes: Option<usize>,
    /// Byte cap for guest stderr capture. Mirror of `stdout_limit_bytes`.
    pub stderr_limit_bytes: Option<usize>,
    /// Isolation posture the frontend requested. The per-invocation
    /// WASI context is built to this rung — `Hermetic` freezes ambient
    /// time and entropy (`crate::ambient`), `Permissive` leaves the
    /// live WASI sources — and `Driver::profile` declares it back as
    /// the built posture.
    pub profile: Profile,
}
