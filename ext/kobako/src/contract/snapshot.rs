//! Engine-neutral, magnus-free per-invocation observable bundle.
//!
//! The success-path outputs of a single guest invocation, expressed
//! without any Ruby type: the outcome bytes and the two captured output
//! channels. Usage is not here — the Runtime stashes it per outcome so it
//! survives the trap path, where no Snapshot is produced. The ext's
//! `Kobako::Snapshot` magnus value wraps one of these and exposes its
//! readers to Ruby.

/// One captured output channel: the bytes the guest wrote (already clipped
/// to the channel's cap) and whether the cap was reached.
pub(crate) struct Capture {
    pub(crate) bytes: Vec<u8>,
    pub(crate) truncated: bool,
}

/// The success-path outputs of one guest invocation: the OUTCOME_BUFFER
/// bytes and the stdout / stderr captures. Usage rides on the Runtime
/// (`last_usage`), not here, so it survives the trap path.
pub(crate) struct Snapshot {
    pub(crate) return_bytes: Vec<u8>,
    pub(crate) stdout: Capture,
    pub(crate) stderr: Capture,
}
