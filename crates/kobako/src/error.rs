//! The SDK's host-facing error taxonomy.
//!
//! Every `Sandbox` invocation either returns a decoded `Value` or
//! exactly one `Error` variant; the variants carry the same
//! attribution the Ruby gem's exception classes do (each variant's
//! doc names its Ruby counterpart), so the differential parity
//! harness can compare the two frontends status-for-status.

use std::fmt;

use kobako_codec::codec::Value;
pub use kobako_runtime::error::SetupError;

/// A guest-side failure decoded from a Panic envelope: the guest
/// exception class and message plus the optional backtrace / details
/// the wire carried.
#[derive(Debug, Clone, PartialEq)]
pub struct GuestFailure {
    pub class: String,
    pub message: String,
    pub backtrace: Vec<String>,
    pub details: Option<Value>,
}

impl fmt::Display for GuestFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.class, self.message)
    }
}

/// What a `Sandbox` invocation raises instead of returning a value.
#[derive(Debug)]
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
    Sandbox(GuestFailure),
    /// Rejected RITE bytecode at replay (Ruby: `Kobako::BytecodeError`).
    Bytecode(GuestFailure),
    /// Service-origin failure — the bound object raised or the
    /// dispatch refused the call (Ruby: `Kobako::ServiceError`).
    Service(GuestFailure),
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
            Error::Setup(setup) => write!(f, "{setup:?}"),
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
}
