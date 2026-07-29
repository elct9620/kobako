//! Per-`Driver` execution configuration.
//!
//! Every cap a frontend forwards into `Driver::new`. A plain value carrier
//! owned by the `Driver` — distinct from the process-wide engine/module
//! `crate::cache` (which is shared across every sandbox) and from the
//! per-invocation `crate::invocation::Invocation` (which the wasm engine
//! mutates from inside a run).
//!
//! Field-for-field the same shape as the SDK's `kobako::Options`, and
//! deliberately not shared with it: this struct is free to name a wasmtime
//! type, and `Options` is not — an engine-free host must still be able to
//! build the SDK.

use std::time::Duration;

use kobako_runtime::profile::Profile;

/// The caps and requested isolation profile for one `Driver`. `None` on
/// any cap field disables that cap.
pub struct Config {
    /// Wall-clock cap for one guest `#eval` / `#run`. Stamped into a
    /// per-run `Instant` deadline by `Driver::prime_caps`.
    pub timeout: Option<Duration>,
    /// Guest linear-memory cap, in bytes. Threaded into each fresh
    /// `Invocation`, where the wasmtime `ResourceLimiter` callback
    /// consumes it from inside the engine.
    pub memory_limit: Option<usize>,
    /// Byte cap for guest stdout capture.
    /// Sizes the per-run `MemoryOutputPipe` and computes the truncation
    /// flag in `Driver::build_snapshot`.
    pub stdout_limit: Option<usize>,
    /// Byte cap for guest stderr capture. Mirror of `stdout_limit`.
    pub stderr_limit: Option<usize>,
    /// Isolation posture the frontend requested. The per-invocation
    /// WASI context is built to this rung — `Hermetic` freezes ambient
    /// time and entropy (`crate::ambient`), `Permissive` leaves the
    /// live WASI sources — and `Driver::profile` declares it back as
    /// the built posture.
    pub profile: Profile,
}
