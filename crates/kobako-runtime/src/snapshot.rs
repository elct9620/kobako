//! Engine-neutral, frontend-free per-invocation observable bundle.
//!
//! The observables of a single guest invocation, expressed without any
//! frontend type and uniform across success and trap: how the invocation
//! completed, the two captured output channels, and the resource usage.
//! A `Snapshot` exists iff the guest export ran — a failure to even start
//! the invocation comes back as the `Err` of `invoke` instead.
//!
//! `Usage` and `Capture` are shared with the tier above rather than
//! translated at its boundary: what they carry is already frontend-free,
//! so the Rust host SDK re-exports these types themselves.

use crate::error::Trap;
use kobako_transport::envelope::Outcome;

/// One captured output channel: the bytes the guest wrote (already clipped
/// to the channel's cap) and whether the cap was reached. `Default` is the
/// pre-invocation sentinel — no bytes, cap not reached — shared by every
/// frontend's before-first-invocation readout.
#[derive(Default)]
#[non_exhaustive]
pub struct Capture {
    pub bytes: Vec<u8>,
    pub truncated: bool,
}

impl Capture {
    /// What a channel the guest wrote `bytes` into keeps under `limit`: the
    /// bytes clipped to it, marked when they passed it. The pipe holds one
    /// byte past the limit, so a channel written to exactly the limit stays
    /// unmarked.
    pub fn new(mut bytes: Vec<u8>, limit: Option<usize>) -> Self {
        let truncated = limit.is_some_and(|limit| bytes.len() > limit);
        if let Some(limit) = limit.filter(|_| truncated) {
            bytes.truncate(limit);
        }
        Self { bytes, truncated }
    }

    /// The room a channel's pipe takes for `limit`: one byte past it, so
    /// writing exactly the limit and passing it stay apart.
    pub fn pipe_capacity_of(limit: Option<usize>) -> usize {
        match limit {
            Some(limit) => limit.saturating_add(1),
            None => usize::MAX,
        }
    }
}

/// How the guest invocation completed: `Outcome` is the result the guest
/// left, already framed; `Trap` is an engine fault after the export call
/// started, kept as a value so the rest of the `Snapshot` survives it.
///
/// A guest that returned without an Outcome the envelope can frame —
/// nothing written included — ends as a `Trap`: with no record to read,
/// there is nothing a frontend could attribute.
///
/// Closed rather than `#[non_exhaustive]`: an invocation either returned
/// a result or failed to, and there is no third way for one to end. A
/// wildcard would only hide a frontend that had not been told otherwise.
pub enum Completion {
    Outcome(Outcome),
    Trap(Trap),
}

/// Resource usage of one guest invocation, each measured across the span
/// its cap governs: `wall_time` is the seconds from setting the guest up to
/// its export call returning, the span the deadline covers; `memory_peak`
/// is the high-water `memory.grow` delta in bytes past the entry-time
/// baseline. `Default` (all zeros) is the pre-invocation sentinel.
#[derive(Clone, Copy, Default)]
#[non_exhaustive]
pub struct Usage {
    pub wall_time: f64,
    pub memory_peak: usize,
}

/// The observables of one guest invocation, uniform across completion
/// kinds: captures and usage are collected on trap and outcome alike.
/// What a frontend exposes from the trap path is its own contract
/// decision.
#[non_exhaustive]
pub struct Snapshot {
    pub completion: Completion,
    pub stdout: Capture,
    pub stderr: Capture,
    pub usage: Usage,
}

impl Snapshot {
    /// What an engine hands back from one invocation.
    pub fn new(completion: Completion, stdout: Capture, stderr: Capture, usage: Usage) -> Self {
        Self {
            completion,
            stdout,
            stderr,
            usage,
        }
    }
}

#[cfg(test)]
mod tests {
    //! Channel-agnostic, a function of `limit` rather than of which channel,
    //! so a regression that breaks one channel cannot slip past.
    use super::Capture;

    fn kept(bytes: &[u8], limit: Option<usize>) -> (Vec<u8>, bool) {
        let capture = Capture::new(bytes.to_vec(), limit);
        (capture.bytes, capture.truncated)
    }

    // @behavior S-108
    #[test]
    fn a_pipe_holds_one_byte_past_its_limit() {
        assert_eq!(
            (
                Capture::pipe_capacity_of(Some(5)),
                Capture::pipe_capacity_of(Some(0))
            ),
            (6, 1),
            "a limit of 5 and of 0 through Capture::pipe_capacity_of must size the pipe 6 and 1"
        );
    }

    // @behavior S-109
    #[test]
    fn a_pipe_without_a_limit_holds_usize_max() {
        assert_eq!(
            Capture::pipe_capacity_of(None),
            usize::MAX,
            "no limit through Capture::pipe_capacity_of must size the pipe usize::MAX"
        );
    }

    // @behavior S-110
    #[test]
    fn a_pipe_past_usize_max_saturates() {
        assert_eq!(
            Capture::pipe_capacity_of(Some(usize::MAX)),
            usize::MAX,
            "a limit of usize::MAX through Capture::pipe_capacity_of must saturate at usize::MAX"
        );
    }

    // @behavior S-111
    #[test]
    fn a_capture_under_its_limit_stays_whole() {
        assert_eq!(
            kept(b"abc", Some(5)),
            (b"abc".to_vec(), false),
            "3 bytes under a limit of 5 through Capture::new must stay whole and untruncated"
        );
    }

    // @behavior S-112
    #[test]
    fn a_capture_at_its_limit_is_not_truncated() {
        assert_eq!(
            kept(b"abcde", Some(5)),
            (b"abcde".to_vec(), false),
            "5 bytes at a limit of 5 through Capture::new must stay whole and untruncated"
        );
    }

    // @behavior S-024
    #[test]
    fn a_capture_past_its_limit_is_clipped_and_truncated() {
        assert_eq!(
            kept(b"abcdef", Some(5)),
            (b"abcde".to_vec(), true),
            "6 bytes past a limit of 5 through Capture::new must clip to 5 and flag truncation"
        );
    }

    // @behavior S-033
    #[test]
    fn a_capture_without_a_limit_stays_whole() {
        assert_eq!(
            kept(b"abcdef", None),
            (b"abcdef".to_vec(), false),
            "6 bytes with no limit through Capture::new must stay whole and untruncated"
        );
    }

    // @behavior S-100 S-101
    #[test]
    fn an_empty_capture_stays_empty() {
        assert_eq!(
            kept(b"", Some(5)),
            (Vec::new(), false),
            "no bytes under a limit of 5 through Capture::new must stay empty and untruncated"
        );
    }
}
