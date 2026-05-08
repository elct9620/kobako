//! Hand-written MessagePack wire codec.
//!
//! Implements the binary encoding pinned by SPEC.md "Wire Codec":
//!
//! * 11 wire types from "Type Mapping" (nil, bool, int, float, str, bin,
//!   array, map, ext + ext 0x01 Handle + ext 0x02 ErrEnv).
//! * Narrowest-encoding rule: each value is encoded with the smallest
//!   msgpack family that fits it (positive fixint before uint 8 before
//!   uint 16, etc.). This is required for byte-identical agreement with
//!   the independent Ruby host implementation under round-trip fuzz
//!   (SPEC.md "Consistency Guarantee").
//! * Two ext codes — 0x01 Handle (fixext 4 + big-endian u32) and
//!   0x02 ErrEnv (ext 8 / ext 16 wrapping an embedded msgpack map).
//! * Specific `WireError` variants on malformed input so the host can
//!   distinguish wire violations from value-level errors.
//!
//! No `unsafe`. No third-party dependencies. Both this crate and the
//! Ruby `lib/kobako/wire/` codec are independent re-implementations of
//! SPEC.md; they end up byte-identical because they both follow SPEC,
//! not because one was copied from the other.

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
    /// preserving the encoded width — the codec always emits float 64.
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

/// MessagePack encoder. Wraps a growable byte buffer.
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

    /// Borrow the encoded bytes without consuming the encoder.
    pub fn as_bytes(&self) -> &[u8] {
        &self.buf
    }

    /// Consume the encoder and return the encoded bytes.
    pub fn into_bytes(self) -> Vec<u8> {
        self.buf
    }

    /// Encode a single `Value` per SPEC.md "Wire Codec".
    ///
    /// The encoder applies the narrowest-encoding rule: integers, strings,
    /// binaries, arrays, and maps each pick the smallest msgpack family
    /// that fits. Two independent implementations of SPEC will end up
    /// byte-identical, which is what makes round-trip fuzz a meaningful
    /// consistency check.
    pub fn write_value(&mut self, value: &Value) -> Result<(), WireError> {
        match value {
            Value::Nil => self.buf.push(0xc0),
            Value::Bool(false) => self.buf.push(0xc2),
            Value::Bool(true) => self.buf.push(0xc3),
            Value::Int(n) => self.write_int(*n),
            Value::UInt(n) => self.write_uint(*n),
            Value::Float(f) => self.write_float(*f),
            Value::Str(s) => self.write_str(s),
            Value::Bin(b) => self.write_bin(b),
            Value::Array(items) => {
                self.write_array_header(items.len())?;
                for item in items {
                    self.write_value(item)?;
                }
            }
            Value::Map(pairs) => {
                self.write_map_header(pairs.len())?;
                for (k, v) in pairs {
                    self.write_value(k)?;
                    self.write_value(v)?;
                }
            }
            Value::Handle(id) => self.write_handle(*id)?,
            Value::ErrEnv(payload) => self.write_errenv(payload)?,
        }
        Ok(())
    }

    fn write_int(&mut self, n: i64) {
        if n >= 0 {
            self.write_uint(n as u64);
            return;
        }
        // n < 0
        if n >= -32 {
            // negative fixint: 0xe0..0xff
            self.buf.push(n as i8 as u8);
        } else if n >= i8::MIN as i64 {
            self.buf.push(0xd0);
            self.buf.push(n as i8 as u8);
        } else if n >= i16::MIN as i64 {
            self.buf.push(0xd1);
            self.buf.extend_from_slice(&(n as i16).to_be_bytes());
        } else if n >= i32::MIN as i64 {
            self.buf.push(0xd2);
            self.buf.extend_from_slice(&(n as i32).to_be_bytes());
        } else {
            self.buf.push(0xd3);
            self.buf.extend_from_slice(&n.to_be_bytes());
        }
    }

    fn write_uint(&mut self, n: u64) {
        if n <= 0x7f {
            // positive fixint
            self.buf.push(n as u8);
        } else if n <= u8::MAX as u64 {
            self.buf.push(0xcc);
            self.buf.push(n as u8);
        } else if n <= u16::MAX as u64 {
            self.buf.push(0xcd);
            self.buf.extend_from_slice(&(n as u16).to_be_bytes());
        } else if n <= u32::MAX as u64 {
            self.buf.push(0xce);
            self.buf.extend_from_slice(&(n as u32).to_be_bytes());
        } else {
            self.buf.push(0xcf);
            self.buf.extend_from_slice(&n.to_be_bytes());
        }
    }

    fn write_float(&mut self, f: f64) {
        // SPEC keeps float width unobserved on the wire; we always emit
        // float 64 so the byte form is deterministic.
        self.buf.push(0xcb);
        self.buf.extend_from_slice(&f.to_be_bytes());
    }

    fn write_str(&mut self, s: &str) {
        let bytes = s.as_bytes();
        let len = bytes.len();
        if len <= 31 {
            // fixstr: 0xa0 | len
            self.buf.push(0xa0 | len as u8);
        } else if len <= u8::MAX as usize {
            self.buf.push(0xd9);
            self.buf.push(len as u8);
        } else if len <= u16::MAX as usize {
            self.buf.push(0xda);
            self.buf.extend_from_slice(&(len as u16).to_be_bytes());
        } else {
            self.buf.push(0xdb);
            self.buf.extend_from_slice(&(len as u32).to_be_bytes());
        }
        self.buf.extend_from_slice(bytes);
    }

    fn write_bin(&mut self, b: &[u8]) {
        let len = b.len();
        if len <= u8::MAX as usize {
            self.buf.push(0xc4);
            self.buf.push(len as u8);
        } else if len <= u16::MAX as usize {
            self.buf.push(0xc5);
            self.buf.extend_from_slice(&(len as u16).to_be_bytes());
        } else {
            self.buf.push(0xc6);
            self.buf.extend_from_slice(&(len as u32).to_be_bytes());
        }
        self.buf.extend_from_slice(b);
    }

    fn write_array_header(&mut self, len: usize) -> Result<(), WireError> {
        if len <= 15 {
            // fixarray
            self.buf.push(0x90 | len as u8);
        } else if len <= u16::MAX as usize {
            self.buf.push(0xdc);
            self.buf.extend_from_slice(&(len as u16).to_be_bytes());
        } else if len <= u32::MAX as usize {
            self.buf.push(0xdd);
            self.buf.extend_from_slice(&(len as u32).to_be_bytes());
        } else {
            return Err(WireError::PayloadTooLarge);
        }
        Ok(())
    }

    fn write_map_header(&mut self, len: usize) -> Result<(), WireError> {
        if len <= 15 {
            // fixmap
            self.buf.push(0x80 | len as u8);
        } else if len <= u16::MAX as usize {
            self.buf.push(0xde);
            self.buf.extend_from_slice(&(len as u16).to_be_bytes());
        } else if len <= u32::MAX as usize {
            self.buf.push(0xdf);
            self.buf.extend_from_slice(&(len as u32).to_be_bytes());
        } else {
            return Err(WireError::PayloadTooLarge);
        }
        Ok(())
    }

    fn write_handle(&mut self, id: u32) -> Result<(), WireError> {
        if id > HANDLE_ID_MAX {
            return Err(WireError::InvalidHandle);
        }
        // fixext 4: 0xd6, type 0x01, big-endian u32 id
        self.buf.push(0xd6);
        self.buf.push(EXT_HANDLE as u8);
        self.buf.extend_from_slice(&id.to_be_bytes());
        Ok(())
    }

    fn write_errenv(&mut self, payload: &[u8]) -> Result<(), WireError> {
        // Encoded payload is an embedded msgpack map. We do not validate
        // the payload structure here — the caller is responsible for
        // having built a valid map. The decoder validates inbound bytes.
        let len = payload.len();
        if len <= u8::MAX as usize {
            // ext 8: 0xc7, 1-byte length, type byte, payload
            self.buf.push(0xc7);
            self.buf.push(len as u8);
            self.buf.push(EXT_ERRENV as u8);
        } else if len <= u16::MAX as usize {
            // ext 16: 0xc8, 2-byte big-endian length, type byte, payload
            self.buf.push(0xc8);
            self.buf.extend_from_slice(&(len as u16).to_be_bytes());
            self.buf.push(EXT_ERRENV as u8);
        } else if len <= u32::MAX as usize {
            // ext 32: 0xc9, 4-byte big-endian length, type byte, payload
            self.buf.push(0xc9);
            self.buf.extend_from_slice(&(len as u32).to_be_bytes());
            self.buf.push(EXT_ERRENV as u8);
        } else {
            return Err(WireError::PayloadTooLarge);
        }
        self.buf.extend_from_slice(payload);
        Ok(())
    }
}

/// MessagePack decoder. Wraps a `&[u8]` cursor.
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
    pub fn read_value(&mut self) -> Result<Value, WireError> {
        let tag = self.read_u8()?;
        match tag {
            // positive fixint 0x00..=0x7f
            0x00..=0x7f => Ok(Value::Int(tag as i64)),
            // fixmap 0x80..=0x8f
            0x80..=0x8f => self.read_map_body((tag & 0x0f) as usize),
            // fixarray 0x90..=0x9f
            0x90..=0x9f => self.read_array_body((tag & 0x0f) as usize),
            // fixstr 0xa0..=0xbf
            0xa0..=0xbf => self.read_str_body((tag & 0x1f) as usize),
            0xc0 => Ok(Value::Nil),
            0xc1 => Err(WireError::InvalidType), // never used
            0xc2 => Ok(Value::Bool(false)),
            0xc3 => Ok(Value::Bool(true)),
            0xc4 => {
                let len = self.read_u8()? as usize;
                self.read_bin_body(len)
            }
            0xc5 => {
                let len = self.read_u16()? as usize;
                self.read_bin_body(len)
            }
            0xc6 => {
                let len = self.read_u32()? as usize;
                self.read_bin_body(len)
            }
            0xc7 => {
                let len = self.read_u8()? as usize;
                let ty = self.read_i8()?;
                self.read_ext_body(ty, len)
            }
            0xc8 => {
                let len = self.read_u16()? as usize;
                let ty = self.read_i8()?;
                self.read_ext_body(ty, len)
            }
            0xc9 => {
                let len = self.read_u32()? as usize;
                let ty = self.read_i8()?;
                self.read_ext_body(ty, len)
            }
            0xca => {
                let bits = self.read_u32()?;
                Ok(Value::Float(f32::from_bits(bits) as f64))
            }
            0xcb => {
                let bits = self.read_u64()?;
                Ok(Value::Float(f64::from_bits(bits)))
            }
            0xcc => Ok(Value::Int(self.read_u8()? as i64)),
            0xcd => Ok(Value::Int(self.read_u16()? as i64)),
            0xce => Ok(Value::Int(self.read_u32()? as i64)),
            0xcf => {
                let n = self.read_u64()?;
                if n <= i64::MAX as u64 {
                    Ok(Value::Int(n as i64))
                } else {
                    Ok(Value::UInt(n))
                }
            }
            0xd0 => Ok(Value::Int(self.read_i8()? as i64)),
            0xd1 => Ok(Value::Int(self.read_i16()? as i64)),
            0xd2 => Ok(Value::Int(self.read_i32()? as i64)),
            0xd3 => Ok(Value::Int(self.read_i64()?)),
            // fixext 1 / 2 / 4 / 8 / 16
            0xd4 => {
                let ty = self.read_i8()?;
                self.read_ext_body(ty, 1)
            }
            0xd5 => {
                let ty = self.read_i8()?;
                self.read_ext_body(ty, 2)
            }
            0xd6 => {
                let ty = self.read_i8()?;
                self.read_ext_body(ty, 4)
            }
            0xd7 => {
                let ty = self.read_i8()?;
                self.read_ext_body(ty, 8)
            }
            0xd8 => {
                let ty = self.read_i8()?;
                self.read_ext_body(ty, 16)
            }
            0xd9 => {
                let len = self.read_u8()? as usize;
                self.read_str_body(len)
            }
            0xda => {
                let len = self.read_u16()? as usize;
                self.read_str_body(len)
            }
            0xdb => {
                let len = self.read_u32()? as usize;
                self.read_str_body(len)
            }
            0xdc => {
                let len = self.read_u16()? as usize;
                self.read_array_body(len)
            }
            0xdd => {
                let len = self.read_u32()? as usize;
                self.read_array_body(len)
            }
            0xde => {
                let len = self.read_u16()? as usize;
                self.read_map_body(len)
            }
            0xdf => {
                let len = self.read_u32()? as usize;
                self.read_map_body(len)
            }
            // negative fixint 0xe0..=0xff
            0xe0..=0xff => Ok(Value::Int(tag as i8 as i64)),
        }
    }

    fn ensure(&self, n: usize) -> Result<(), WireError> {
        if self.pos.checked_add(n).map_or(true, |end| end > self.input.len()) {
            Err(WireError::Truncated)
        } else {
            Ok(())
        }
    }

    fn read_u8(&mut self) -> Result<u8, WireError> {
        self.ensure(1)?;
        let b = self.input[self.pos];
        self.pos += 1;
        Ok(b)
    }

    fn read_i8(&mut self) -> Result<i8, WireError> {
        Ok(self.read_u8()? as i8)
    }

    fn read_u16(&mut self) -> Result<u16, WireError> {
        self.ensure(2)?;
        let b: [u8; 2] = self.input[self.pos..self.pos + 2].try_into().unwrap();
        self.pos += 2;
        Ok(u16::from_be_bytes(b))
    }

    fn read_i16(&mut self) -> Result<i16, WireError> {
        Ok(self.read_u16()? as i16)
    }

    fn read_u32(&mut self) -> Result<u32, WireError> {
        self.ensure(4)?;
        let b: [u8; 4] = self.input[self.pos..self.pos + 4].try_into().unwrap();
        self.pos += 4;
        Ok(u32::from_be_bytes(b))
    }

    fn read_i32(&mut self) -> Result<i32, WireError> {
        Ok(self.read_u32()? as i32)
    }

    fn read_u64(&mut self) -> Result<u64, WireError> {
        self.ensure(8)?;
        let b: [u8; 8] = self.input[self.pos..self.pos + 8].try_into().unwrap();
        self.pos += 8;
        Ok(u64::from_be_bytes(b))
    }

    fn read_i64(&mut self) -> Result<i64, WireError> {
        Ok(self.read_u64()? as i64)
    }

    fn read_bytes(&mut self, n: usize) -> Result<Vec<u8>, WireError> {
        self.ensure(n)?;
        let v = self.input[self.pos..self.pos + n].to_vec();
        self.pos += n;
        Ok(v)
    }

    fn read_str_body(&mut self, len: usize) -> Result<Value, WireError> {
        let bytes = self.read_bytes(len)?;
        let s = String::from_utf8(bytes).map_err(|_| WireError::Utf8)?;
        Ok(Value::Str(s))
    }

    fn read_bin_body(&mut self, len: usize) -> Result<Value, WireError> {
        Ok(Value::Bin(self.read_bytes(len)?))
    }

    fn read_array_body(&mut self, len: usize) -> Result<Value, WireError> {
        let mut items = Vec::with_capacity(len);
        for _ in 0..len {
            items.push(self.read_value()?);
        }
        Ok(Value::Array(items))
    }

    fn read_map_body(&mut self, len: usize) -> Result<Value, WireError> {
        let mut pairs = Vec::with_capacity(len);
        for _ in 0..len {
            let k = self.read_value()?;
            let v = self.read_value()?;
            pairs.push((k, v));
        }
        Ok(Value::Map(pairs))
    }

    fn read_ext_body(&mut self, ty: i8, len: usize) -> Result<Value, WireError> {
        match ty {
            EXT_HANDLE => {
                if len != 4 {
                    return Err(WireError::InvalidHandle);
                }
                self.ensure(4)?;
                let b: [u8; 4] = self.input[self.pos..self.pos + 4].try_into().unwrap();
                self.pos += 4;
                let id = u32::from_be_bytes(b);
                if id > HANDLE_ID_MAX {
                    return Err(WireError::InvalidHandle);
                }
                Ok(Value::Handle(id))
            }
            EXT_ERRENV => {
                let payload = self.read_bytes(len)?;
                // Validate that the payload is itself a msgpack map. We
                // peek without consuming the outer cursor.
                let mut inner = Decoder::new(&payload);
                match inner.read_value() {
                    Ok(Value::Map(_)) if inner.at_end() => {}
                    _ => return Err(WireError::InvalidErrEnv),
                }
                Ok(Value::ErrEnv(payload))
            }
            _ => Err(WireError::InvalidType),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(v: Value) -> Value {
        let mut enc = Encoder::new();
        enc.write_value(&v).expect("encode");
        let bytes = enc.into_bytes();
        let mut dec = Decoder::new(&bytes);
        let out = dec.read_value().expect("decode");
        assert!(dec.at_end(), "decoder must consume all bytes");
        out
    }

    fn encode(v: &Value) -> Vec<u8> {
        let mut enc = Encoder::new();
        enc.write_value(v).expect("encode");
        enc.into_bytes()
    }

    // ------------------------------------------------------------------
    // Skeleton invariants (preserved from item #4)
    // ------------------------------------------------------------------

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
        let bytes = [0xc0_u8];
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
        assert_eq!(EXT_HANDLE, 0x01);
        assert_eq!(EXT_ERRENV, 0x02);
    }

    #[test]
    fn outcome_tags_match_spec() {
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

    // ------------------------------------------------------------------
    // Spec-derived golden vectors (encoder is locked to SPEC bytes)
    // ------------------------------------------------------------------

    #[test]
    fn golden_nil() {
        assert_eq!(encode(&Value::Nil), vec![0xc0]);
    }

    #[test]
    fn golden_bool_false() {
        assert_eq!(encode(&Value::Bool(false)), vec![0xc2]);
    }

    #[test]
    fn golden_bool_true() {
        assert_eq!(encode(&Value::Bool(true)), vec![0xc3]);
    }

    #[test]
    fn golden_int_zero() {
        assert_eq!(encode(&Value::Int(0)), vec![0x00]);
    }

    #[test]
    fn golden_int_neg_one() {
        assert_eq!(encode(&Value::Int(-1)), vec![0xff]);
    }

    #[test]
    fn golden_int_127_is_positive_fixint() {
        assert_eq!(encode(&Value::Int(127)), vec![0x7f]);
    }

    #[test]
    fn golden_int_neg_32_is_negative_fixint() {
        assert_eq!(encode(&Value::Int(-32)), vec![0xe0]);
    }

    #[test]
    fn golden_empty_string() {
        assert_eq!(encode(&Value::Str(String::new())), vec![0xa0]);
    }

    #[test]
    fn golden_empty_array() {
        assert_eq!(encode(&Value::Array(Vec::new())), vec![0x90]);
    }

    #[test]
    fn golden_empty_map() {
        assert_eq!(encode(&Value::Map(Vec::new())), vec![0x80]);
    }

    #[test]
    fn golden_handle_zero_is_fixext4() {
        // SPEC.md "Ext Types" → ext 0x01: 0xd6 marker, 0x01 type, 4-byte BE id.
        // ID 0 is the invalid sentinel but is wire-legal at the codec level
        // (rejection happens at the HandleTable layer per SPEC).
        assert_eq!(encode(&Value::Handle(0)), vec![0xd6, 0x01, 0x00, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn golden_handle_max() {
        assert_eq!(
            encode(&Value::Handle(HANDLE_ID_MAX)),
            vec![0xd6, 0x01, 0x7f, 0xff, 0xff, 0xff]
        );
    }

    #[test]
    fn golden_outcome_result_42() {
        // SPEC.md "Outcome Envelope" example: tag 0x01 + fixarray len=1 + 42.
        let mut buf = vec![OUTCOME_TAG_RESULT];
        let mut enc = Encoder::new();
        enc.write_value(&Value::Array(vec![Value::Int(42)])).unwrap();
        buf.extend(enc.into_bytes());
        assert_eq!(buf, vec![0x01, 0x91, 0x2a]);
    }

    // ------------------------------------------------------------------
    // Narrowest-encoding rule
    // ------------------------------------------------------------------

    #[test]
    fn narrowest_int_boundaries() {
        // Positive fixint upper bound.
        assert_eq!(encode(&Value::Int(127))[0], 0x7f);
        // Just past fixint -> uint 8.
        assert_eq!(encode(&Value::Int(128))[0], 0xcc);
        // Just past u8 -> uint 16.
        assert_eq!(encode(&Value::Int(256))[0], 0xcd);
        // Just past u16 -> uint 32.
        assert_eq!(encode(&Value::Int(65_536))[0], 0xce);
        // Just past u32 -> uint 64.
        assert_eq!(encode(&Value::Int(0x1_0000_0000))[0], 0xcf);

        // Negative fixint lower bound.
        assert_eq!(encode(&Value::Int(-32))[0], 0xe0);
        // Just past negative fixint -> int 8.
        assert_eq!(encode(&Value::Int(-33))[0], 0xd0);
        // Just past i8 -> int 16.
        assert_eq!(encode(&Value::Int(-129))[0], 0xd1);
        // Just past i16 -> int 32.
        assert_eq!(encode(&Value::Int(-32_769))[0], 0xd2);
        // Just past i32 -> int 64.
        assert_eq!(encode(&Value::Int(-2_147_483_649))[0], 0xd3);
    }

    #[test]
    fn narrowest_uint_max_uses_uint64() {
        assert_eq!(encode(&Value::UInt(u64::MAX))[0], 0xcf);
    }

    #[test]
    fn narrowest_str_boundaries() {
        assert_eq!(encode(&Value::Str("a".repeat(31)))[0], 0xbf);
        assert_eq!(encode(&Value::Str("a".repeat(32)))[0], 0xd9);
        assert_eq!(encode(&Value::Str("a".repeat(256)))[0], 0xda);
        assert_eq!(encode(&Value::Str("a".repeat(65_536)))[0], 0xdb);
    }

    #[test]
    fn narrowest_bin_boundaries() {
        assert_eq!(encode(&Value::Bin(vec![0u8; 255]))[0], 0xc4);
        assert_eq!(encode(&Value::Bin(vec![0u8; 256]))[0], 0xc5);
        assert_eq!(encode(&Value::Bin(vec![0u8; 65_536]))[0], 0xc6);
    }

    #[test]
    fn narrowest_array_boundaries() {
        assert_eq!(encode(&Value::Array(vec![Value::Nil; 15]))[0], 0x9f);
        assert_eq!(encode(&Value::Array(vec![Value::Nil; 16]))[0], 0xdc);
    }

    #[test]
    fn narrowest_map_boundaries() {
        let m15: Vec<(Value, Value)> = (0..15)
            .map(|i| (Value::Int(i), Value::Nil))
            .collect();
        assert_eq!(encode(&Value::Map(m15))[0], 0x8f);
        let m16: Vec<(Value, Value)> = (0..16)
            .map(|i| (Value::Int(i), Value::Nil))
            .collect();
        assert_eq!(encode(&Value::Map(m16))[0], 0xde);
    }

    // ------------------------------------------------------------------
    // Round-trip coverage for every wire type, including boundary values
    // ------------------------------------------------------------------

    #[test]
    fn roundtrip_nil_and_bools() {
        assert_eq!(roundtrip(Value::Nil), Value::Nil);
        assert_eq!(roundtrip(Value::Bool(true)), Value::Bool(true));
        assert_eq!(roundtrip(Value::Bool(false)), Value::Bool(false));
    }

    #[test]
    fn roundtrip_int_boundaries() {
        let cases: &[i64] = &[
            0,
            1,
            -1,
            127,
            128,
            -32,
            -33,
            i8::MIN as i64,
            i8::MIN as i64 - 1,
            i16::MAX as i64,
            i16::MIN as i64,
            i16::MIN as i64 - 1,
            i32::MAX as i64,
            i32::MIN as i64,
            i32::MIN as i64 - 1,
            i64::MAX,
            i64::MIN,
            255,
            256,
            65_535,
            65_536,
            0xffff_ffff,
            0x1_0000_0000,
        ];
        for &n in cases {
            assert_eq!(roundtrip(Value::Int(n)), Value::Int(n), "Int({})", n);
        }
    }

    #[test]
    fn roundtrip_uint_max_preserves_uint_variant() {
        assert_eq!(roundtrip(Value::UInt(u64::MAX)), Value::UInt(u64::MAX));
        // i64::MAX as u64 must come back as Int (positive fits in i64).
        assert_eq!(
            roundtrip(Value::UInt(i64::MAX as u64)),
            Value::Int(i64::MAX)
        );
    }

    #[test]
    fn roundtrip_floats_special() {
        // Positive zero.
        match roundtrip(Value::Float(0.0)) {
            Value::Float(f) => {
                assert_eq!(f, 0.0);
                assert!(f.is_sign_positive());
            }
            other => panic!("unexpected: {:?}", other),
        }
        // Negative zero.
        match roundtrip(Value::Float(-0.0)) {
            Value::Float(f) => {
                assert_eq!(f, 0.0);
                assert!(f.is_sign_negative());
            }
            other => panic!("unexpected: {:?}", other),
        }
        // Infinities.
        match roundtrip(Value::Float(f64::INFINITY)) {
            Value::Float(f) => assert!(f.is_infinite() && f.is_sign_positive()),
            other => panic!("unexpected: {:?}", other),
        }
        match roundtrip(Value::Float(f64::NEG_INFINITY)) {
            Value::Float(f) => assert!(f.is_infinite() && f.is_sign_negative()),
            other => panic!("unexpected: {:?}", other),
        }
        // NaN.
        match roundtrip(Value::Float(f64::NAN)) {
            Value::Float(f) => assert!(f.is_nan()),
            other => panic!("unexpected: {:?}", other),
        }
        // Ordinary value.
        assert_eq!(
            roundtrip(Value::Float(std::f64::consts::PI)),
            Value::Float(std::f64::consts::PI)
        );
    }

    #[test]
    fn roundtrip_str_lengths_and_multibyte() {
        let cases = vec![
            String::new(),
            "a".to_string(),
            "a".repeat(31),
            "a".repeat(32),
            "a".repeat(255),
            "a".repeat(256),
            "a".repeat(65_535),
            "a".repeat(65_536),
            // Multibyte UTF-8.
            "蒼時弦也".to_string(),
            "🌸 cherry blossom".to_string(),
        ];
        for s in cases {
            assert_eq!(roundtrip(Value::Str(s.clone())), Value::Str(s));
        }
    }

    #[test]
    fn roundtrip_bin_with_non_utf8_bytes() {
        let cases = vec![
            Vec::<u8>::new(),
            vec![0x00, 0xff, 0xfe, 0x80, 0xc0],
            vec![0xff; 255],
            vec![0xab; 256],
            vec![0xcd; 65_536],
        ];
        for b in cases {
            assert_eq!(roundtrip(Value::Bin(b.clone())), Value::Bin(b));
        }
    }

    #[test]
    fn roundtrip_arrays_empty_and_nested() {
        assert_eq!(roundtrip(Value::Array(vec![])), Value::Array(vec![]));
        let nested = Value::Array(vec![
            Value::Int(1),
            Value::Array(vec![Value::Bool(true), Value::Nil]),
            Value::Str("x".into()),
        ]);
        assert_eq!(roundtrip(nested.clone()), nested);

        // Length 16 to cross fixarray boundary.
        let big = Value::Array((0..16).map(Value::Int).collect());
        assert_eq!(roundtrip(big.clone()), big);
    }

    #[test]
    fn roundtrip_maps_empty_and_nested() {
        assert_eq!(roundtrip(Value::Map(vec![])), Value::Map(vec![]));
        let m = Value::Map(vec![
            (Value::Str("a".into()), Value::Int(1)),
            (
                Value::Str("nested".into()),
                Value::Map(vec![(Value::Str("k".into()), Value::Bool(false))]),
            ),
        ]);
        assert_eq!(roundtrip(m.clone()), m);
    }

    #[test]
    fn roundtrip_handle_boundaries() {
        assert_eq!(roundtrip(Value::Handle(0)), Value::Handle(0));
        assert_eq!(roundtrip(Value::Handle(1)), Value::Handle(1));
        assert_eq!(
            roundtrip(Value::Handle(HANDLE_ID_MAX)),
            Value::Handle(HANDLE_ID_MAX)
        );
    }

    #[test]
    fn roundtrip_errenv_payload() {
        // Build a valid embedded msgpack map: {"type" => "runtime",
        // "message" => "boom", "details" => nil}.
        let mut inner = Encoder::new();
        inner
            .write_value(&Value::Map(vec![
                (Value::Str("type".into()), Value::Str("runtime".into())),
                (Value::Str("message".into()), Value::Str("boom".into())),
                (Value::Str("details".into()), Value::Nil),
            ]))
            .unwrap();
        let payload = inner.into_bytes();
        let v = Value::ErrEnv(payload.clone());
        assert_eq!(roundtrip(v), Value::ErrEnv(payload));
    }

    #[test]
    fn roundtrip_deeply_nested_mixed() {
        let inner_errenv = {
            let mut e = Encoder::new();
            e.write_value(&Value::Map(vec![
                (Value::Str("type".into()), Value::Str("argument".into())),
                (Value::Str("message".into()), Value::Str("bad".into())),
            ]))
            .unwrap();
            Value::ErrEnv(e.into_bytes())
        };
        let v = Value::Array(vec![
            Value::Handle(7),
            Value::Map(vec![
                (Value::Str("xs".into()), Value::Array(vec![
                    Value::Int(1),
                    Value::Int(-1),
                    Value::Float(2.5),
                ])),
                (Value::Str("err".into()), inner_errenv),
                (Value::Str("blob".into()), Value::Bin(vec![0, 1, 2, 3])),
            ]),
        ]);
        assert_eq!(roundtrip(v.clone()), v);
    }

    // ------------------------------------------------------------------
    // Decoder error variants are specific
    // ------------------------------------------------------------------

    #[test]
    fn decode_truncated_input_returns_truncated() {
        // 0xa3 = fixstr len=3, but no following bytes.
        let bytes = [0xa3];
        let mut dec = Decoder::new(&bytes);
        assert_eq!(dec.read_value(), Err(WireError::Truncated));

        // uint 16 marker but only one length byte present.
        let bytes = [0xcd, 0x00];
        let mut dec = Decoder::new(&bytes);
        assert_eq!(dec.read_value(), Err(WireError::Truncated));

        // Empty input.
        let mut dec = Decoder::new(&[]);
        assert_eq!(dec.read_value(), Err(WireError::Truncated));
    }

    #[test]
    fn decode_invalid_type_tag_returns_invalid_type() {
        // 0xc1 is reserved/never used in msgpack.
        let bytes = [0xc1];
        let mut dec = Decoder::new(&bytes);
        assert_eq!(dec.read_value(), Err(WireError::InvalidType));

        // fixext 1 with an unknown ext code (0x05 — not 0x01 or 0x02).
        let bytes = [0xd4, 0x05, 0x00];
        let mut dec = Decoder::new(&bytes);
        assert_eq!(dec.read_value(), Err(WireError::InvalidType));
    }

    #[test]
    fn decode_invalid_utf8_in_str_returns_utf8() {
        // fixstr len=2 with bytes that are not valid UTF-8 (a lone 0xff
        // followed by a low byte).
        let bytes = [0xa2, 0xff, 0xfe];
        let mut dec = Decoder::new(&bytes);
        assert_eq!(dec.read_value(), Err(WireError::Utf8));
    }

    #[test]
    fn decode_handle_with_wrong_payload_length_returns_invalid_handle() {
        // fixext 1 with type 0x01 — Handle with only 1 payload byte.
        let bytes = [0xd4, 0x01, 0x00];
        let mut dec = Decoder::new(&bytes);
        assert_eq!(dec.read_value(), Err(WireError::InvalidHandle));
    }

    #[test]
    fn decode_handle_above_cap_returns_invalid_handle() {
        // fixext 4 with type 0x01 and id = 0x80000000 (one above cap).
        let bytes = [0xd6, 0x01, 0x80, 0x00, 0x00, 0x00];
        let mut dec = Decoder::new(&bytes);
        assert_eq!(dec.read_value(), Err(WireError::InvalidHandle));
    }

    #[test]
    fn decode_errenv_with_non_map_payload_returns_invalid_errenv() {
        // ext 8, length 1, type 0x02, payload 0xc0 (nil — not a map).
        let bytes = [0xc7, 0x01, 0x02, 0xc0];
        let mut dec = Decoder::new(&bytes);
        assert_eq!(dec.read_value(), Err(WireError::InvalidErrEnv));
    }

    #[test]
    fn decode_uint64_above_i64max_uses_uint_variant() {
        // 0xcf (uint 64) followed by 0xffffffff_ffffffff (u64::MAX).
        let mut bytes = vec![0xcf];
        bytes.extend_from_slice(&u64::MAX.to_be_bytes());
        let mut dec = Decoder::new(&bytes);
        assert_eq!(dec.read_value(), Ok(Value::UInt(u64::MAX)));
    }

    #[test]
    fn decode_accepts_float32_payload() {
        // 0xca (float 32) marker — encoder emits float 64 only, but the
        // decoder must still accept float 32 from a peer that picks the
        // narrower form.
        let mut bytes = vec![0xca];
        bytes.extend_from_slice(&1.5f32.to_be_bytes());
        let mut dec = Decoder::new(&bytes);
        match dec.read_value() {
            Ok(Value::Float(f)) => assert_eq!(f, 1.5),
            other => panic!("unexpected: {:?}", other),
        }
    }
}
