//! Whether a test reading the built Guest Binaries may go on. The unit tests
//! here and the integration tests of this crate and the SDK each compile
//! apart, so each reads this one file.

/// Whether a test reading the Guest Binaries at `what` may go on, `present`
/// saying they were built: one missing skips the test locally and fails it
/// under CI, which always builds them.
pub(crate) fn built(present: bool, what: &str) -> bool {
    assert!(
        present || std::env::var_os("CI").is_none(),
        "{what} missing under CI — run `bundle exec rake wasm:build`"
    );
    present
}
