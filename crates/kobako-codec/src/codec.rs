//! MessagePack codec — Rust-side glue over the `rmp` crate.
//!
//! The kobako codec (docs/wire-codec.md) is plain MessagePack with
//! three ext type codes — 0x00 Symbol (variable-length ext carrying the
//! symbol name as UTF-8 bytes), 0x01 Capability Handle (`fixext 4`,
//! big-endian u32) and 0x02 Exception envelope (variable-length ext
//! wrapping an embedded msgpack map). The Ruby host encodes through the
//! official `msgpack` gem; the Rust side encodes through `rmp` here.
//! Both pickers apply MessagePack's narrowest-encoding rule, which
//! keeps the two implementations byte-aligned without any cross-language
//! sharing.
//!
//! This module is a thin shim: the public surface — `Value`, `Encoder`,
//! `Decoder`, `Error` — carries the whole wire byte form, and every
//! consumer (the envelope files here, the guest-ABI machinery in
//! `kobako-core`, the round-trip oracle binary) reaches the bytes only
//! through it; the byte-level work is delegated to `rmp::encode` /
//! `rmp::decode`. One file per responsibility — `error`, `value`,
//! `encoder`, `decoder` — each re-exported at this root so call sites
//! name it `codec::Value` etc.

pub mod decoder;
pub mod encoder;
pub mod error;
pub mod value;

pub use decoder::{Decode, Decoder};
pub use encoder::{Encode, Encoder};
pub use error::Error;
pub use value::Value;

/// MessagePack ext type code reserved for Symbol (docs/wire-codec.md
/// § Ext Types → ext 0x00). Module-private — mirrors the `EXT_SYMBOL`
/// constant on the Ruby Factory side.
const EXT_SYMBOL: i8 = 0x00;

/// MessagePack ext type code reserved for Capability Handle
/// (docs/wire-codec.md § Ext Types → ext 0x01). Module-private — every
/// encoder/decoder that needs it lives inside this module.
const EXT_HANDLE: i8 = 0x01;

/// MessagePack ext type code reserved for Exception envelope
/// (docs/wire-codec.md § Ext Types → ext 0x02). Module-private — every
/// encoder/decoder that needs it lives inside this module.
const EXT_ERRENV: i8 = 0x02;

/// Maximum legal Capability Handle ID (docs/wire-codec.md § Ext Types
/// → ext 0x01). Module-private.
const HANDLE_ID_MAX: u32 = 0x7fff_ffff;

/// Maximum structural nesting depth the guest codec walks on both the
/// encode and decode paths (docs/wire-codec.md § Structural Nesting
/// Depth). The cap keeps a reference cycle or a pathologically deep
/// payload from overflowing the wasm stack and hard-trapping the guest;
/// it sits far below that overflow threshold and matches the limit the
/// host's MessagePack library imposes, so both sides reject the same
/// boundary. Shared with the encode-side walk in `kobako`'s codec
/// conversion so the two guest walks cannot drift apart.
pub const MAX_NESTING_DEPTH: usize = 128;
