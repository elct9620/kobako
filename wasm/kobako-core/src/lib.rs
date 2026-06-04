//! kobako-core — Guest ABI contract crate root.
//!
//! Language-agnostic building blocks for a kobako Guest Binary,
//! mirroring the host's `lib/kobako/` wire tiers (SPEC.md "Wire
//! Codec"). mruby never enters this crate; the assembled mruby guest
//! and any third-party guest build on it alike.

/// Width in bytes of the length prefix that precedes each stdin frame
/// and outcome buffer (docs/wire-codec.md § Invocation channels).
pub const FRAME_LEN_SIZE: usize = 4;

pub mod abi;
pub mod codec;
pub mod outcome;
pub mod transport;
