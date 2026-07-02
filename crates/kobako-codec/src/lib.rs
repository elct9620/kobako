//! kobako-codec — portable wire tier crate root.
//!
//! The Rust expression of the kobako Transport wire (SPEC.md "Wire
//! Codec"), shared by both sides of the wasm boundary: `codec` is the
//! MessagePack byte codec, `transport` the Request / Response / Yield
//! envelope value objects, `outcome` the per-invocation Outcome /
//! Panic records. The guest-ABI contract crate (`kobako-core`) builds
//! its transport machinery on this tier; a Rust host encodes the same
//! envelopes with it directly. Nothing here is guest-bound — no ABI
//! import, no mruby, no engine.

/// Width in bytes of the length prefix that precedes each stdin frame
/// and outcome buffer (docs/wire-codec.md § Invocation channels).
pub const FRAME_LEN_SIZE: usize = 4;

pub mod codec;
pub mod outcome;
pub mod transport;
