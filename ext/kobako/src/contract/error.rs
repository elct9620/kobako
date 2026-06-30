//! Engine-neutral host error channels, free of any `magnus` dependency.
//!
//! The run path produces these instead of constructing Ruby exceptions
//! directly; the ext boundary (`crate::runtime::errors`) is the single place
//! that maps them onto the `Kobako::*` classes. Keeping the channels
//! magnus-free lets the run mechanics move to a standalone runtime crate
//! unchanged.

use std::fmt;

/// A guest invocation that faulted in the wasm engine, or a host-detected
/// runtime corruption during invocation, classified into the host-facing
/// kinds. The boundary maps `Timeout` to `Kobako::TimeoutError`,
/// `MemoryLimit` to `Kobako::MemoryLimitError`, and `Other` to
/// `Kobako::TrapError`.
#[derive(Debug)]
pub(crate) enum Trap {
    Timeout(String),
    MemoryLimit(String),
    Other(String),
}

/// A failure that yields no invocation outcome. The discriminant records the
/// runtime's state so the boundary can pick the SPEC-assigned class:
/// `ModuleNotBuilt` (artifact absent) maps to `Kobako::ModuleNotBuiltError`,
/// `Dead` (runtime could not be constructed) to `Kobako::SetupError`, and
/// `Intact` (runtime live, a host-side pre-call step failed) to
/// `Kobako::SandboxError`.
#[derive(Debug)]
pub(crate) enum SetupError {
    ModuleNotBuilt(String),
    Dead(String),
    Intact(String),
}

/// Either run-path channel, unified so the run mechanics can propagate both
/// with `?`. The boundary destructures it back into the two channels.
#[derive(Debug)]
pub(crate) enum Error {
    Trap(Trap),
    Setup(SetupError),
}

impl From<Trap> for Error {
    fn from(trap: Trap) -> Self {
        Error::Trap(trap)
    }
}

impl From<SetupError> for Error {
    fn from(err: SetupError) -> Self {
        Error::Setup(err)
    }
}

impl fmt::Display for Trap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (Trap::Timeout(msg) | Trap::MemoryLimit(msg) | Trap::Other(msg)) = self;
        f.write_str(msg)
    }
}
