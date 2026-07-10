//! Engine-neutral, frontend-free per-invocation observable bundle.
//!
//! The observables of a single guest invocation, expressed without any
//! frontend type and uniform across success and trap: how the invocation
//! completed, the two captured output channels, and the resource usage.
//! A `Snapshot` exists iff the guest export ran — a failure to even start
//! the invocation travels on the `invoke` `Err` channel instead.

use crate::error::Trap;

/// One captured output channel: the bytes the guest wrote (already clipped
/// to the channel's cap) and whether the cap was reached. `Default` is the
/// pre-invocation sentinel — no bytes, cap not reached — shared by every
/// frontend's before-first-invocation readout.
#[derive(Default)]
pub struct Capture {
    pub bytes: Vec<u8>,
    pub truncated: bool,
}

/// How the guest invocation completed: `Outcome` carries the
/// OUTCOME_BUFFER bytes the guest returned; `Trap` is an engine fault
/// after the export call started, kept as a value so the rest of the
/// `Snapshot` survives it.
pub enum Completion {
    Outcome(Vec<u8>),
    Trap(Trap),
}

/// Resource usage of one guest invocation, measured across the same
/// bracket as the wall-clock / memory caps: `wall_time` is the seconds
/// spent inside the guest export call; `memory_peak` is the high-water
/// `memory.grow` delta in bytes past the entry-time baseline. `Default`
/// (all zeros) is the pre-invocation sentinel.
#[derive(Clone, Copy, Default)]
pub struct Usage {
    pub wall_time: f64,
    pub memory_peak: usize,
}

/// The observables of one guest invocation, uniform across completion
/// kinds: captures and usage are collected on trap and outcome alike.
/// What a frontend exposes from the trap path is its own contract
/// decision.
pub struct Snapshot {
    pub completion: Completion,
    pub stdout: Capture,
    pub stderr: Capture,
    pub usage: Usage,
}
