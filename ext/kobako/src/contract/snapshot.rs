//! Engine-neutral, magnus-free per-invocation observable bundle.
//!
//! Everything a single guest invocation produces, expressed without any
//! Ruby type: the outcome bytes, the two captured output channels, and the
//! resource usage. The ext's `Kobako::Snapshot` magnus value wraps one of
//! these and exposes its readers to Ruby.

use std::time::Duration;

/// One captured output channel: the bytes the guest wrote (already clipped
/// to the channel's cap) and whether the cap was reached.
pub(crate) struct Capture {
    pub(crate) bytes: Vec<u8>,
    pub(crate) truncated: bool,
}

/// Per-invocation resource usage — the figures `Sandbox#usage` surfaces.
pub(crate) struct Usage {
    pub(crate) wall_time: Duration,
    pub(crate) memory_peak: usize,
}

/// Everything observable from one guest invocation: the OUTCOME_BUFFER
/// bytes, the stdout / stderr captures, and the usage figures.
pub(crate) struct Snapshot {
    pub(crate) return_bytes: Vec<u8>,
    pub(crate) stdout: Capture,
    pub(crate) stderr: Capture,
    pub(crate) usage: Usage,
}
