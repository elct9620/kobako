//! Per-channel stdout / stderr capture sizing and clipping.
//!
//! Two pure helpers shared by the run path: one
//! sizes the per-run `MemoryOutputPipe`, the other clips a captured
//! snapshot back to the configured cap and reports whether the cap was
//! exceeded. Kept channel-agnostic (a function of `cap`, not of which
//! channel) so a regression that only breaks one channel cannot sneak
//! through the test that pins them.

/// Translate a per-channel byte cap into the MemoryOutputPipe capacity:
/// `cap + 1` (saturated against `usize::MAX`) when a cap is set so the
/// "wrote exactly cap" and "exceeded cap" cases stay distinguishable;
/// `usize::MAX` when the channel is uncapped.
pub(super) fn pipe_capacity(cap: Option<usize>) -> usize {
    match cap {
        Some(c) => c.saturating_add(1),
        None => usize::MAX,
    }
}

/// Pure slicing core shared by the snapshot readback: given the unclipped
/// pipe snapshot and the configured cap, return the bytes Ruby should
/// observe (clipped to `cap`) plus the truncation flag. `truncated` is
/// `true` only when the snapshot strictly exceeded the cap — this is the
/// "wrote `cap + 1` bytes into a `cap + 1`-sized pipe" case; "wrote
/// exactly `cap` bytes" stays `false`.
pub(super) fn clip_capture(raw: &[u8], cap: Option<usize>) -> (&[u8], bool) {
    match cap {
        Some(c) if raw.len() > c => (&raw[..c], true),
        _ => (raw, false),
    }
}

#[cfg(test)]
mod tests {
    use super::{clip_capture, pipe_capacity};

    #[test]
    fn pipe_capacity_adds_one_when_cap_is_set() {
        assert_eq!(pipe_capacity(Some(5)), 6);
        assert_eq!(pipe_capacity(Some(0)), 1);
    }

    #[test]
    fn pipe_capacity_falls_back_to_usize_max_when_uncapped() {
        assert_eq!(pipe_capacity(None), usize::MAX);
    }

    #[test]
    fn pipe_capacity_saturates_at_usize_max() {
        assert_eq!(pipe_capacity(Some(usize::MAX)), usize::MAX);
    }

    #[test]
    fn clip_capture_returns_full_bytes_when_under_cap() {
        let (bytes, truncated) = clip_capture(b"abc", Some(5));
        assert_eq!(bytes, b"abc");
        assert!(!truncated);
    }

    #[test]
    fn clip_capture_does_not_flag_truncation_at_exactly_cap_bytes() {
        let (bytes, truncated) = clip_capture(b"abcde", Some(5));
        assert_eq!(bytes, b"abcde");
        assert!(!truncated);
    }

    #[test]
    fn clip_capture_clips_to_cap_and_flags_truncation_on_overflow() {
        // The pipe is sized `cap + 1`, so the snapshot can be at most
        // 6 bytes when `cap == 5`; that surface is what triggers the
        // truncation flag.
        let (bytes, truncated) = clip_capture(b"abcdef", Some(5));
        assert_eq!(bytes, b"abcde");
        assert!(truncated);
    }

    #[test]
    fn clip_capture_treats_none_as_uncapped() {
        let (bytes, truncated) = clip_capture(b"abcdef", None);
        assert_eq!(bytes, b"abcdef");
        assert!(!truncated);
    }

    #[test]
    fn clip_capture_handles_empty_input() {
        let (bytes, truncated) = clip_capture(b"", Some(5));
        assert_eq!(bytes, b"");
        assert!(!truncated);
    }
}
