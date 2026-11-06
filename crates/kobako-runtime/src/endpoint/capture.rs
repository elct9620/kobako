//! Per-channel stdout / stderr capture sizing and clipping.
//!
//! Two pure helpers shared by the run path: one sizes the per-run pipe, the
//! other clips a captured snapshot back to the configured limit and reports
//! whether the limit was passed. Kept channel-agnostic (a function of `limit`,
//! not of which channel) so a regression that only breaks one channel cannot
//! sneak through the test that pins them.

/// One byte past the limit, so "wrote exactly the limit" and "passed the
/// limit" stay distinguishable.
pub fn pipe_capacity(limit: Option<usize>) -> usize {
    match limit {
        Some(limit) => limit.saturating_add(1),
        None => usize::MAX,
    }
}

/// Takes the snapshot by value and truncates in place, so the readback
/// costs one copy out of the pipe rather than two.
pub fn clip_capture(mut raw: Vec<u8>, limit: Option<usize>) -> (Vec<u8>, bool) {
    match limit {
        Some(limit) if raw.len() > limit => {
            raw.truncate(limit);
            (raw, true)
        }
        _ => (raw, false),
    }
}

#[cfg(test)]
mod tests {
    use super::{clip_capture, pipe_capacity};

    // @behavior S-108
    #[test]
    fn a_pipe_holds_one_byte_past_its_limit() {
        assert_eq!(
            (pipe_capacity(Some(5)), pipe_capacity(Some(0))),
            (6, 1),
            "a limit of 5 and of 0 through pipe_capacity must size the pipe 6 and 1"
        );
    }

    // @behavior S-109
    #[test]
    fn a_pipe_without_a_limit_holds_usize_max() {
        assert_eq!(
            pipe_capacity(None),
            usize::MAX,
            "no limit through pipe_capacity must size the pipe usize::MAX"
        );
    }

    // @behavior S-110
    #[test]
    fn a_pipe_past_usize_max_saturates() {
        assert_eq!(
            pipe_capacity(Some(usize::MAX)),
            usize::MAX,
            "a limit of usize::MAX through pipe_capacity must saturate at usize::MAX"
        );
    }

    // @behavior S-111
    #[test]
    fn a_capture_under_its_limit_stays_whole() {
        assert_eq!(
            clip_capture(b"abc".to_vec(), Some(5)),
            (b"abc".to_vec(), false),
            "3 bytes under a limit of 5 through clip_capture must stay whole and untruncated"
        );
    }

    // @behavior S-112
    #[test]
    fn a_capture_at_its_limit_is_not_truncated() {
        assert_eq!(
            clip_capture(b"abcde".to_vec(), Some(5)),
            (b"abcde".to_vec(), false),
            "5 bytes at a limit of 5 through clip_capture must stay whole and untruncated"
        );
    }

    // @behavior S-024
    #[test]
    fn a_capture_past_its_limit_is_clipped_and_truncated() {
        // The pipe holds one byte past the limit, so 6 bytes is the most a
        // limit of 5 can capture.
        assert_eq!(
            clip_capture(b"abcdef".to_vec(), Some(5)),
            (b"abcde".to_vec(), true),
            "6 bytes past a limit of 5 through clip_capture must clip to 5 and flag truncation"
        );
    }

    // @behavior S-033
    #[test]
    fn a_capture_without_a_limit_stays_whole() {
        assert_eq!(
            clip_capture(b"abcdef".to_vec(), None),
            (b"abcdef".to_vec(), false),
            "6 bytes with no limit through clip_capture must stay whole and untruncated"
        );
    }

    // @behavior S-100 S-101
    #[test]
    fn an_empty_capture_stays_empty() {
        assert_eq!(
            clip_capture(Vec::new(), Some(5)),
            (Vec::new(), false),
            "no bytes under a limit of 5 through clip_capture must stay empty and untruncated"
        );
    }
}
