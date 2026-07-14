//! The codec error type and the `rmp` error mappings that feed it.

use rmp::decode::{MarkerReadError, NumValueReadError, ValueReadError};

/// Errors raised by the codec when bytes do not conform to the kobako
/// codec (docs/wire-codec.md). The byte-level variants cover a value that
/// is the wrong msgpack family or truncated; `Malformed` covers a value
/// that decoded cleanly but whose higher structure is wrong (a message
/// with the wrong arity, a missing required field, a field of the wrong
/// type). The host raises both through a single `Codec::Error`, so per
/// SPEC the host need not distinguish the two when reporting a
/// wire-contract violation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Truncated,
    InvalidType,
    Utf8,
    InvalidHandle,
    InvalidErrEnv,
    PayloadTooLarge,
    /// Decoded as a valid msgpack value, but its structure violates the
    /// expected shape. The message is a self-contained description of
    /// what was expected (e.g. "Request must be a 5-element array").
    Malformed(&'static str),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Truncated => f.write_str("truncated msgpack input"),
            Error::InvalidType => f.write_str("invalid msgpack type for kobako codec"),
            Error::Utf8 => f.write_str("invalid UTF-8 in msgpack str"),
            Error::InvalidHandle => f.write_str("invalid Capability Handle (ext 0x01)"),
            Error::InvalidErrEnv => f.write_str("invalid Exception envelope (ext 0x02)"),
            Error::PayloadTooLarge => f.write_str("msgpack payload exceeds u32 length"),
            Error::Malformed(msg) => write!(f, "malformed msgpack structure: {msg}"),
        }
    }
}

impl std::error::Error for Error {}

// ---------------------------------------------------------------------------
// rmp error mapping
// ---------------------------------------------------------------------------

impl<E: rmp::decode::RmpReadErr> From<ValueReadError<E>> for Error {
    fn from(err: ValueReadError<E>) -> Self {
        match err {
            ValueReadError::InvalidMarkerRead(_) | ValueReadError::InvalidDataRead(_) => {
                Error::Truncated
            }
            ValueReadError::TypeMismatch(_) => Error::InvalidType,
        }
    }
}

impl<E: rmp::decode::RmpReadErr> From<MarkerReadError<E>> for Error {
    fn from(_: MarkerReadError<E>) -> Self {
        Error::Truncated
    }
}

impl<E: rmp::decode::RmpReadErr> From<NumValueReadError<E>> for Error {
    fn from(err: NumValueReadError<E>) -> Self {
        match err {
            NumValueReadError::InvalidMarkerRead(_) | NumValueReadError::InvalidDataRead(_) => {
                Error::Truncated
            }
            NumValueReadError::TypeMismatch(_) | NumValueReadError::OutOfRange => {
                Error::InvalidType
            }
        }
    }
}
