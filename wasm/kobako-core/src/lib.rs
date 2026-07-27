//! kobako-core — Guest ABI contract crate root.
//!
//! Language-agnostic building blocks for a kobako Guest Binary: the
//! `Guest` trait + `export_guest!` macro turn the ABI export
//! enumeration into a compiler-checked contract, and `abi` / `frames`
//! / `transport::proxy` carry the guest-bound machinery behind it.
//! The messages themselves — the envelopes and the ABI's own values —
//! live in `kobako-transport`, this crate's only dependency and the tier
//! every kobako assembly shares. No payload codec enters here: routing a
//! message never reads one, which is what lets a guest speaking its own
//! schema build on this crate unchanged. mruby never enters either; the
//! assembled mruby guest and any third-party guest build on it alike.

pub mod abi;
pub mod frames;
mod guest;
pub mod transport;

pub use guest::Guest;
