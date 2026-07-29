//! Engine-neutral host error channels, free of any frontend dependency.
//!
//! The run path produces these instead of constructing host-language
//! exceptions directly; each frontend's boundary is the single place that
//! maps them onto its own error classes (the Ruby ext does so in its error
//! mapper). Keeping the channels frontend-free lets any engine
//! implementation produce them unchanged.

use std::fmt;

/// A guest invocation that faulted in the wasm engine, or a host-detected
/// runtime corruption during invocation, classified into the host-facing
/// kinds a frontend surfaces distinctly: the wall-clock cap (`Timeout`),
/// the linear-memory cap (`MemoryLimit`), and every other engine fault
/// (`Other`).
#[derive(Debug)]
pub enum Trap {
    Timeout(String),
    MemoryLimit(String),
    Other(String),
}

/// A failure that yields no invocation outcome. The discriminant records
/// the runtime's state so a frontend can attribute the failure per SPEC:
/// `ModuleNotBuilt` (the guest artifact is absent), `Dead` (the runtime
/// could not be constructed), and `Intact` (the runtime is live but a
/// host-side pre-call step failed, so no discard-and-recreate recovery is
/// owed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetupError {
    ModuleNotBuilt(String),
    Dead(String),
    Intact(String),
}

/// A failure that prevented the invocation from starting — a pre-call
/// engine fault (`Trap`) or a host-side setup fault (`Setup`) — unified so
/// the run mechanics can propagate both with `?`; a frontend destructures
/// it back into the two channels. Faults after the guest export starts
/// ride in `Completion::Trap` instead, so captures and usage survive them.
///
/// Named for where it comes from — the `Err` of `Runtime::invoke` — so a
/// host that also has its own `Error` in scope can hold both.
#[derive(Debug)]
pub enum InvokeError {
    Trap(Trap),
    Setup(SetupError),
}

impl From<Trap> for InvokeError {
    fn from(trap: Trap) -> Self {
        InvokeError::Trap(trap)
    }
}

impl From<SetupError> for InvokeError {
    fn from(err: SetupError) -> Self {
        InvokeError::Setup(err)
    }
}

impl fmt::Display for InvokeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InvokeError::Trap(trap) => trap.fmt(f),
            InvokeError::Setup(err) => err.fmt(f),
        }
    }
}

impl fmt::Display for Trap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (Trap::Timeout(msg) | Trap::MemoryLimit(msg) | Trap::Other(msg)) = self;
        f.write_str(msg)
    }
}

impl fmt::Display for SetupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (SetupError::ModuleNotBuilt(msg) | SetupError::Dead(msg) | SetupError::Intact(msg)) =
            self;
        f.write_str(msg)
    }
}

// Both channels sit on the seam a third-party engine implements against,
// where a caller wants to lift them into `Box<dyn Error>` or `anyhow` with
// `?`. `Trap` carries no source of its own, so neither reports one.
impl std::error::Error for SetupError {}

impl std::error::Error for InvokeError {}
