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

/// Allocation cap on a length-prefixed frame: a prefix beyond any
/// legitimate frame is rejected before the payload is allocated. Sits
/// well above SPEC's 16 MiB single-call payload bound so every frame
/// reader and oracle applies the same ceiling.
pub const MAX_FRAME_LEN: usize = 64 * 1024 * 1024;

pub mod codec;
pub mod outcome;
pub mod transport;
