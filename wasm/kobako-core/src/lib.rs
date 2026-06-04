//! kobako-core — Guest ABI contract crate root.
//!
//! Language-agnostic building blocks for a kobako Guest Binary,
//! mirroring the host's `lib/kobako/` wire tiers (SPEC.md "Wire
//! Codec"). mruby never enters this crate; the assembled mruby guest
//! and any third-party guest build on it alike.

pub mod codec;
pub mod outcome;
