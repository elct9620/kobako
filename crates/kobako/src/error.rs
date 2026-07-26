//! The SDK's host-facing error taxonomy.
//!
//! One `Error` enum serves two positions, partitioned by whether the
//! guest ran. As the outer `Err` of `eval` / `run` it is a run that
//! never started (`Setup` / `Sealed` / `Argument`, plus a pre-call
//! Handle exhaustion). Inside an `Execution::outcome` it is a run that
//! started and failed — a trap or a guest-origin failure. The variants
//! carry the same attribution the Ruby gem's exception classes do (each
//! variant's doc names its Ruby counterpart), so the differential parity
//! harness compares the two frontends status-for-status whichever
//! position the failure lands in.

use std::fmt;

pub use kobako_runtime::error::SetupError;

/// One record of a failed invocation: the exception class, message and
/// backtrace the wire carried, the names the invocation could have used in
/// place of the one it named, and — when the host itself detected the
/// violation — the codec detail behind it.
///
/// Each `Error` variant names where the failure came from, so the record
/// itself only has to say what failed.
///
/// Non-exhaustive because a Panic gains fields as the wire does, and an
/// embedder reads this rather than building one — so a later field is a
/// wire change, not a break in this API.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Failure {
    pub class: String,
    pub message: String,
    pub backtrace: Vec<String>,
    /// Empty unless the failure offers a correction — the top-level
    /// constants an unresolved `#run` entrypoint could have been.
    pub available: Vec<String>,
    /// The codec fault behind a host-detected wire violation. Kept out of
    /// `message`, which names the failure a caller can act on, and carried
    /// here for the operator triaging a corrupted runtime.
    pub diagnostic: Option<String>,
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.class, self.message)
    }
}

/// What a `Sandbox` invocation raises instead of returning a value.
///
/// Non-exhaustive because the taxonomy grows append-only alongside
/// the SPEC error anchors; match the variants you handle and keep a
/// wildcard arm for the ones a future kobako adds.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// The wall-clock cap fired (Ruby: `Kobako::TimeoutError`).
    Timeout(String),
    /// The linear-memory cap fired (Ruby: `Kobako::MemoryLimitError`).
    MemoryLimit(String),
    /// Any other engine trap, or an absent / corrupt outcome buffer
    /// (Ruby: `Kobako::TrapError`).
    Trap(String),
    /// Guest-origin failure — uncaught exception, compile failure, or
    /// a wire violation (Ruby: `Kobako::SandboxError`).
    Sandbox(Box<Failure>),
    /// Rejected RITE bytecode at replay (Ruby: `Kobako::BytecodeError`).
    Bytecode(Box<Failure>),
    /// Service-origin failure — the bound object raised or the
    /// dispatch refused the call (Ruby: `Kobako::ServiceError`).
    Service(Box<Failure>),
    /// The invocation never started: guest artifact absent or
    /// unusable, or a host-side pre-call step failed.
    Setup(SetupError),
    /// A registration verb arrived after the first invocation sealed
    /// the Sandbox's tables.
    Sealed(&'static str),
    /// A host-side pre-flight refusal — malformed snippet or entrypoint
    /// name, duplicate snippet name, or unencodable arguments (Ruby:
    /// `ArgumentError`).
    Argument(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Timeout(msg) | Error::MemoryLimit(msg) | Error::Trap(msg) => f.write_str(msg),
            Error::Argument(msg) => f.write_str(msg),
            Error::Sandbox(failure) | Error::Bytecode(failure) | Error::Service(failure) => {
                write!(f, "{failure}")
            }
            Error::Setup(setup) => write!(f, "{setup}"),
            Error::Sealed(what) => write!(f, "Sandbox is sealed; {what}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<kobako_runtime::error::Error> for Error {
    /// Fold the contract's pre-call channel: a pre-call trap keeps its
    /// cap attribution, a setup fault stays `Setup`.
    fn from(err: kobako_runtime::error::Error) -> Self {
        match err {
            kobako_runtime::error::Error::Trap(trap) => trap.into(),
            kobako_runtime::error::Error::Setup(setup) => Error::Setup(setup),
        }
    }
}

impl From<kobako_runtime::error::Trap> for Error {
    fn from(trap: kobako_runtime::error::Trap) -> Self {
        match trap {
            kobako_runtime::error::Trap::Timeout(msg) => Error::Timeout(msg),
            kobako_runtime::error::Trap::MemoryLimit(msg) => Error::MemoryLimit(msg),
            kobako_runtime::error::Trap::Other(msg) => Error::Trap(msg),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The trap folding is the one mapping with room to drift: each cap
    // must keep its own attribution instead of collapsing into `Trap`.
    #[test]
    fn trap_channels_keep_their_cap_attribution() {
        assert!(matches!(
            Error::from(kobako_runtime::error::Trap::Timeout("t".into())),
            Error::Timeout(_)
        ));
        assert!(matches!(
            Error::from(kobako_runtime::error::Trap::MemoryLimit("m".into())),
            Error::MemoryLimit(_)
        ));
        assert!(matches!(
            Error::from(kobako_runtime::error::Trap::Other("o".into())),
            Error::Trap(_)
        ));
    }

    #[test]
    fn contract_error_setup_stays_setup() {
        let err = kobako_runtime::error::Error::Setup(SetupError::Intact("pre-call".into()));
        assert!(matches!(Error::from(err), Error::Setup(_)));
    }

    // A Setup error displayed to a host embedder must read as the plain
    // failure message, not the leaked `ModuleNotBuilt("…")` Debug form.
    #[test]
    fn setup_error_display_is_the_plain_message() {
        let err = Error::Setup(SetupError::ModuleNotBuilt("kobako.wasm not found".into()));
        assert_eq!(err.to_string(), "kobako.wasm not found");
    }
}
