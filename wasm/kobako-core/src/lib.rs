//! kobako-core — Guest ABI contract crate root.
//!
//! Language-agnostic building blocks for a kobako Guest Binary,
//! mirroring the host's `lib/kobako/` wire tiers (SPEC.md "Wire
//! Codec"): the `Guest` trait + `export_guest!` macro turn the ABI
//! export enumeration into a compiler-checked contract, and `codec` /
//! `transport` / `outcome` / `abi` carry the wire machinery behind
//! it. mruby never enters this crate; the assembled mruby guest and
//! any third-party guest build on it alike.

/// Width in bytes of the length prefix that precedes each stdin frame
/// and outcome buffer (docs/wire-codec.md § Invocation channels).
pub const FRAME_LEN_SIZE: usize = 4;

pub mod abi;
pub mod codec;
mod guest;
pub mod outcome;
pub mod transport;

pub use guest::Guest;
