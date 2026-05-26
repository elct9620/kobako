//! Kobako transport layer — guest-side mirror of the host's
//! `lib/kobako/transport/` directory. One file per value object —
//! [`Request`] (`request`), [`Response`] (`response`), [`Yield`] (`block`)
//! — plus the guest dispatch path (`proxy`). Each type is re-exported at
//! this root so call sites name it `transport::Request` etc., matching the
//! host's flat `Kobako::Transport::Request`.
//!
//! Each per-call envelope type carries its own wire codec through the
//! [`Encode`] / [`Decode`] traits — the Rust-native expression of the
//! shared contract the Ruby host gets via duck typing (`#encode` /
//! `.decode` on each value object). Both halves return [`crate::codec::Error`]:
//! a value object is encoded or decoded as a whole, and any byte-level or
//! structural fault surfaces through the one codec error type. Types that
//! only travel one direction (e.g. the host→guest invocation envelope)
//! implement only the half they need.

pub mod block;
pub mod proxy;
pub mod request;
pub mod response;

pub use block::{Yield, TAG_BREAK, TAG_ERROR, TAG_OK, TAG_RESERVED};
pub use request::{Request, Target};
pub use response::Response;

/// Encode a per-call transport envelope to its wire bytes. The value
/// object's own invariants are the contract; this does not re-validate
/// the shape.
pub trait Encode {
    fn encode(&self) -> Result<Vec<u8>, crate::codec::Error>;
}

/// Decode wire bytes into a per-call transport envelope. Returns
/// [`crate::codec::Error::Malformed`] when the bytes parse as a value but
/// do not match the expected envelope shape.
pub trait Decode: Sized {
    fn decode(bytes: &[u8]) -> Result<Self, crate::codec::Error>;
}
