//! kobako-core — Guest ABI contract crate root.
//!
//! Language-agnostic building blocks for a kobako Guest Binary: the
//! `Guest` trait + `export_guest!` macro turn the ABI export
//! enumeration into a compiler-checked contract, and `abi` / `frames`
//! / `transport::proxy` carry the guest-bound machinery behind it.
//! The portable wire tier — codec, envelopes, outcome — lives in the
//! `kobako-codec` crate, which this crate builds on and which mirrors
//! the host's `lib/kobako/` wire tiers (SPEC.md "Wire Codec"). mruby
//! never enters this crate; the assembled mruby guest and any
//! third-party guest build on it alike.

pub mod abi;
pub mod frames;
mod guest;
pub mod transport;

pub use guest::Guest;
