//! Hand-written MessagePack wire codec — skeleton.
//!
//! This module pins the public surface of the codec. Encode/decode bodies
//! are placeholders (`unimplemented!()`) until item #6 fills them in. The
//! shape here is deliberately minimal:
//!
//! * `Value` — sum type covering the 11 wire types from SPEC.md "Type
//!   Mapping". Two ext variants (`Handle` for ext 0x01, `ErrEnv` for ext
//!   0x02) are modelled as named variants rather than a generic ext bag,
//!   because SPEC.md "Ext Types" pins these as the only legal ext codes;
//!   any other ext code is a wire violation that the decoder rejects.
//! * `Encoder` / `Decoder` — wrappers around an in-memory byte buffer /
//!   slice. Construction and primitive accessors work; encode/decode of
//!   `Value` is a stub.
//! * `WireError` — error type returned by the decoder. Variants align with
//!   the wire-violation classes named in SPEC.md "Error Scenarios" →
//!   `Kobako::TrapError` and "Wire Codec".
//!
//! No `unsafe`. No third-party dependencies. This file is deliberately
//! self-contained so that item #6's TDD cycles can extend a minimal,
//! review-friendly surface.

// Codec uses `std` on every target; see lib.rs for rationale.

/// MessagePack ext type code reserved for Capability Handle (SPEC.md
/// "Ext Types" → ext 0x01). Encoded as `fixext 4` with a big-endian u32
/// payload.
pub const EXT_HANDLE: i8 = 0x01;

/// MessagePack ext type code reserved for Exception envelope (SPEC.md
/// "Ext Types" → ext 0x02). Encoded as `ext 8` / `ext 16` wrapping an
/// embedded msgpack map with keys `type`, `message`, `details`.
pub const EXT_ERRENV: i8 = 0x02;

/// Outcome envelope tag for a Result envelope (SPEC.md "Outcome Envelope").
pub const OUTCOME_TAG_RESULT: u8 = 0x01;

/// Outcome envelope tag for a Panic envelope (SPEC.md "Outcome Envelope").
pub const OUTCOME_TAG_PANIC: u8 = 0x02;

/// Maximum legal Capability Handle ID (SPEC.md "Ext Types" → ext 0x01).
/// IDs above this cap are wire violations.
pub const HANDLE_ID_MAX: u32 = 0x7fff_ffff;

/// Single-RPC payload size limit (SPEC.md "ABI Signatures"): 16 MiB.
pub const MAX_PAYLOAD_BYTES: usize = 16 * 1024 * 1024;

/// Errors raised by the codec when input bytes do not conform to the kobako
/// wire (SPEC.md "Wire Codec"). On the host side these surface as
/// `Kobako::SandboxError` (wire violation in a Response) or
/// `Kobako::TrapError` (wire violation in the Outcome envelope).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WireError {
    /// Input ended before a complete msgpack value could be decoded.
    Truncated,
    /// A msgpack format byte not recognised by the kobako wire (e.g. a
    /// timestamp ext, an unknown ext code other than 0x01 / 0x02, or a
    /// reserved msgpack format byte).
    InvalidType,
    /// A `str` family value contained bytes that are not valid UTF-8, or
    /// a `bin`-encoded map key was expected to be UTF-8 and was not.
    Utf8,
    /// A Capability Handle payload had a length other than 4 bytes, or
    /// an ID above `HANDLE_ID_MAX`, or the reserved invalid-sentinel ID 0
    /// in a position that requires a live Handle.
    InvalidHandle,
    /// An Exception envelope payload was not a msgpack map, or was missing
    /// one of the required keys (`type`, `message`).
    InvalidErrEnv,
    /// Payload exceeds the 16 MiB single-RPC limit (SPEC.md "ABI
    /// Signatures").
    PayloadTooLarge,
}

/// A decoded msgpack value, restricted to the 11 wire types accepted on the
/// kobako wire (SPEC.md "Type Mapping"). Any msgpack value outside this set
/// is rejected at decode time with `WireError::InvalidType`.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// `nil` — wire type #1.
    Nil,
    /// `bool` — wire type #2.
    Bool(bool),
    /// `int` (any width). Stored as `i64` because msgpack's signed ints
    /// reach i64::MIN; uint 64 values that exceed i64::MAX are represented
    /// via the `UInt` variant below.
    Int(i64),
    /// `uint 64` values that do not fit in `i64`. Kept distinct so that
    /// round-trip fuzz can recover the original encoding family.
    UInt(u64),
    /// `float` (32 or 64). Stored as `f64`; SPEC.md does not require
    /// preserving the encoded width.
    Float(f64),
    /// `str` family — UTF-8 text (SPEC.md "str / bin Encoding Rules").
    Str(String),
    /// `bin` family — arbitrary bytes.
    Bin(Vec<u8>),
    /// `array` family.
    Array(Vec<Value>),
    /// `map` family. Stored as a `Vec` of pairs rather than a `HashMap`
    /// because SPEC.md "Wire Codec" treats key order as wire-observable
    /// for fuzz round-trip purposes.
    Map(Vec<(Value, Value)>),
    /// ext 0x01 Capability Handle (SPEC.md "Ext Types" → ext 0x01).
    Handle(u32),
    /// ext 0x02 Exception envelope (SPEC.md "Ext Types" → ext 0x02). The
    /// payload is an embedded msgpack map; we keep it as raw bytes here
    /// and let the boot script decode the inner map after recognising
    /// the ext code.
    ErrEnv(Vec<u8>),
}

/// Encoder skeleton. Wraps a growable byte buffer; item #6 will add the
/// per-type write methods. The public surface intentionally matches a
/// future zero-allocation streaming style — `into_bytes` consumes the
/// encoder so callers do not retain mutable aliases.
#[derive(Debug, Default)]
pub struct Encoder {
    buf: Vec<u8>,
}

impl Encoder {
    /// Create an empty encoder.
    pub fn new() -> Self {
        Self { buf: Vec::new() }
    }

    /// Create an encoder with a pre-sized buffer.
    pub fn with_capacity(cap: usize) -> Self {
        Self {
            buf: Vec::with_capacity(cap),
        }
    }

    /// Encoded byte length so far.
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    /// True if no bytes have been written yet.
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// Consume the encoder and return the encoded bytes.
    pub fn into_bytes(self) -> Vec<u8> {
        self.buf
    }

    /// Encode a single `Value` per SPEC.md "Wire Codec".
    ///
    /// Item #6 will fill this in. Calling it on the skeleton panics with
    /// `unimplemented!()` to make accidental early use loud.
    pub fn write_value(&mut self, _value: &Value) -> Result<(), WireError> {
        unimplemented!("Encoder::write_value is delivered by item #6")
    }
}

/// Decoder skeleton. Wraps a `&[u8]` cursor; item #6 will add per-type
/// read methods. The decoder borrows from the input, but `Value` owns its
/// `String` / `Vec<u8>` / nested `Value` data so that envelopes can be
/// passed across function boundaries.
#[derive(Debug)]
pub struct Decoder<'a> {
    input: &'a [u8],
    pos: usize,
}

impl<'a> Decoder<'a> {
    /// Wrap an input byte slice.
    pub fn new(input: &'a [u8]) -> Self {
        Self { input, pos: 0 }
    }

    /// Total length of the underlying input.
    pub fn len(&self) -> usize {
        self.input.len()
    }

    /// True if the input is empty.
    pub fn is_empty(&self) -> bool {
        self.input.is_empty()
    }

    /// Current cursor position (bytes consumed).
    pub fn position(&self) -> usize {
        self.pos
    }

    /// True if every byte has been consumed.
    pub fn at_end(&self) -> bool {
        self.pos >= self.input.len()
    }

    /// Decode a single `Value` per SPEC.md "Wire Codec".
    ///
    /// Item #6 will fill this in. Calling it on the skeleton panics with
    /// `unimplemented!()` to make accidental early use loud.
    pub fn read_value(&mut self) -> Result<Value, WireError> {
        unimplemented!("Decoder::read_value is delivered by item #6")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoder_starts_empty() {
        let enc = Encoder::new();
        assert!(enc.is_empty());
        assert_eq!(enc.len(), 0);
        assert!(enc.into_bytes().is_empty());
    }

    #[test]
    fn encoder_with_capacity_is_still_empty() {
        let enc = Encoder::with_capacity(64);
        assert!(enc.is_empty());
        assert_eq!(enc.len(), 0);
    }

    #[test]
    fn decoder_tracks_position() {
        let bytes = [0xc0_u8]; // msgpack nil
        let dec = Decoder::new(&bytes);
        assert_eq!(dec.position(), 0);
        assert_eq!(dec.len(), 1);
        assert!(!dec.is_empty());
        assert!(!dec.at_end());
    }

    #[test]
    fn decoder_empty_input_is_at_end() {
        let dec = Decoder::new(&[]);
        assert!(dec.is_empty());
        assert!(dec.at_end());
    }

    #[test]
    fn ext_codes_match_spec() {
        // SPEC.md "Ext Types" pins these. Locking them at the test layer
        // catches accidental rewrites of the constants.
        assert_eq!(EXT_HANDLE, 0x01);
        assert_eq!(EXT_ERRENV, 0x02);
    }

    #[test]
    fn outcome_tags_match_spec() {
        // SPEC.md "Outcome Envelope" pins these.
        assert_eq!(OUTCOME_TAG_RESULT, 0x01);
        assert_eq!(OUTCOME_TAG_PANIC, 0x02);
    }

    #[test]
    fn handle_id_cap_matches_spec() {
        assert_eq!(HANDLE_ID_MAX, (1u32 << 31) - 1);
    }

    #[test]
    fn payload_limit_is_16_mib() {
        assert_eq!(MAX_PAYLOAD_BYTES, 16 * 1024 * 1024);
    }

    #[test]
    fn value_variants_cover_eleven_wire_types() {
        // Smoke test: every wire variant constructs. This guards against
        // someone deleting a variant during refactoring without also
        // deleting the corresponding decode path.
        let _ = Value::Nil;
        let _ = Value::Bool(true);
        let _ = Value::Int(-1);
        let _ = Value::UInt(u64::MAX);
        let _ = Value::Float(1.5);
        let _ = Value::Str(String::from("x"));
        let _ = Value::Bin(Vec::new());
        let _ = Value::Array(Vec::new());
        let _ = Value::Map(Vec::new());
        let _ = Value::Handle(1);
        let _ = Value::ErrEnv(Vec::new());
    }
}
