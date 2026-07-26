//! kobako-codec — portable wire tier crate root.
//!
//! The Rust expression of the kobako Transport wire (SPEC.md "Wire
//! Codec"), shared by both sides of the wasm boundary: `envelope` is the
//! core envelope, and each payload codec gets a namespace named after its
//! schema — `msgpack` holds the byte codec and the invocation arguments
//! it carries. The guest-ABI contract crate (`kobako-core`) builds its
//! transport machinery on this tier; a Rust host encodes the same
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

// The core envelope needs nothing from a payload codec, so it is the only
// module a `--no-default-features` build keeps.
pub mod envelope;

#[cfg(feature = "msgpack")]
pub mod msgpack;
