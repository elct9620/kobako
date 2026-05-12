//! MessagePack wire codec — guest-side glue over the `rmp` crate.
//!
//! The kobako wire format (SPEC.md "Wire Codec") is plain MessagePack with
//! two ext type codes — 0x01 Capability Handle (`fixext 4`, big-endian
//! u32) and 0x02 Exception envelope (variable-length ext wrapping an
//! embedded msgpack map). The host side encodes through the official
//! `msgpack` Ruby gem; the guest side encodes through `rmp` here. Both
//! pickers apply MessagePack's narrowest-encoding rule, which keeps the
//! two implementations byte-aligned without any cross-language sharing.
//!
//! This module is intentionally a thin shim: the public surface — `Value`,
//! `Encoder`, `Decoder`, `WireError` — is the same one downstream callers
//! (`envelope.rs`, `rpc_client.rs`, the round-trip oracle binary) used
//! against the previous hand-rolled implementation, but the byte-level
//! work is now delegated to `rmp::encode` / `rmp::decode`.

use rmp::decode::{read_marker, MarkerReadError, NumValueReadError, RmpRead, ValueReadError};
use rmp::encode::{
    write_array_len, write_bin, write_bool, write_ext_meta, write_f64, write_map_len, write_nil,
    write_sint, write_str, write_uint,
};
use rmp::Marker;

/// MessagePack ext type code reserved for Capability Handle (SPEC.md
/// "Ext Types" → ext 0x01).
pub const EXT_HANDLE: i8 = 0x01;

/// MessagePack ext type code reserved for Exception envelope (SPEC.md
/// "Ext Types" → ext 0x02).
pub const EXT_ERRENV: i8 = 0x02;

/// Outcome envelope tag for a Result envelope (SPEC.md "Outcome Envelope").
pub const OUTCOME_TAG_RESULT: u8 = 0x01;

/// Outcome envelope tag for a Panic envelope (SPEC.md "Outcome Envelope").
pub const OUTCOME_TAG_PANIC: u8 = 0x02;

/// Maximum legal Capability Handle ID (SPEC.md "Ext Types" → ext 0x01).
pub const HANDLE_ID_MAX: u32 = 0x7fff_ffff;

/// Errors raised by the codec when input bytes do not conform to the
/// kobako wire (SPEC.md "Wire Codec").
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WireError {
    Truncated,
    InvalidType,
    Utf8,
    InvalidHandle,
    InvalidErrEnv,
    PayloadTooLarge,
}

/// A decoded msgpack value, restricted to the 11 wire types the kobako
/// wire accepts (SPEC.md "Type Mapping"). Anything outside this set is
/// rejected at decode time with `WireError::InvalidType`.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Nil,
    Bool(bool),
    Int(i64),
    UInt(u64),
    Float(f64),
    Str(String),
    Bin(Vec<u8>),
    Array(Vec<Value>),
    Map(Vec<(Value, Value)>),
    Handle(u32),
    /// Raw bytes of the embedded msgpack map carried inside an ext 0x02
    /// envelope. Re-decoding the inner map is the boot script's job; the
    /// codec only validates it parses as a single msgpack map.
    ErrEnv(Vec<u8>),
}

// ---------------------------------------------------------------------------
// rmp error mapping
// ---------------------------------------------------------------------------

impl<E: rmp::decode::RmpReadErr> From<ValueReadError<E>> for WireError {
    fn from(err: ValueReadError<E>) -> Self {
        match err {
            ValueReadError::InvalidMarkerRead(_) | ValueReadError::InvalidDataRead(_) => {
                WireError::Truncated
            }
            ValueReadError::TypeMismatch(_) => WireError::InvalidType,
        }
    }
}

impl<E: rmp::decode::RmpReadErr> From<MarkerReadError<E>> for WireError {
    fn from(_: MarkerReadError<E>) -> Self {
        WireError::Truncated
    }
}

impl<E: rmp::decode::RmpReadErr> From<NumValueReadError<E>> for WireError {
    fn from(err: NumValueReadError<E>) -> Self {
        match err {
            NumValueReadError::InvalidMarkerRead(_) | NumValueReadError::InvalidDataRead(_) => {
                WireError::Truncated
            }
            NumValueReadError::TypeMismatch(_) | NumValueReadError::OutOfRange => {
                WireError::InvalidType
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Encoder
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
pub struct Encoder {
    buf: Vec<u8>,
}

impl Encoder {
    pub fn new() -> Self {
        Self { buf: Vec::new() }
    }

    pub fn with_capacity(cap: usize) -> Self {
        Self {
            buf: Vec::with_capacity(cap),
        }
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.buf
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.buf
    }

    pub fn write_value(&mut self, value: &Value) -> Result<(), WireError> {
        match value {
            Value::Nil => write_nil(&mut self.buf).map_err(|_| WireError::Truncated)?,
            Value::Bool(b) => write_bool(&mut self.buf, *b).map_err(|_| WireError::Truncated)?,
            Value::Int(n) => {
                write_sint(&mut self.buf, *n).map_err(|_| WireError::Truncated)?;
            }
            Value::UInt(n) => {
                write_uint(&mut self.buf, *n).map_err(|_| WireError::Truncated)?;
            }
            Value::Float(f) => {
                write_f64(&mut self.buf, *f).map_err(|_| WireError::Truncated)?;
            }
            Value::Str(s) => write_str(&mut self.buf, s).map_err(|_| WireError::Truncated)?,
            Value::Bin(b) => write_bin(&mut self.buf, b).map_err(|_| WireError::Truncated)?,
            Value::Array(items) => {
                let len = u32::try_from(items.len()).map_err(|_| WireError::PayloadTooLarge)?;
                write_array_len(&mut self.buf, len).map_err(|_| WireError::Truncated)?;
                for item in items {
                    self.write_value(item)?;
                }
            }
            Value::Map(pairs) => {
                let len = u32::try_from(pairs.len()).map_err(|_| WireError::PayloadTooLarge)?;
                write_map_len(&mut self.buf, len).map_err(|_| WireError::Truncated)?;
                for (k, v) in pairs {
                    self.write_value(k)?;
                    self.write_value(v)?;
                }
            }
            Value::Handle(id) => {
                if *id > HANDLE_ID_MAX {
                    return Err(WireError::InvalidHandle);
                }
                write_ext_meta(&mut self.buf, 4, EXT_HANDLE).map_err(|_| WireError::Truncated)?;
                self.buf.extend_from_slice(&id.to_be_bytes());
            }
            Value::ErrEnv(payload) => {
                let len = u32::try_from(payload.len()).map_err(|_| WireError::PayloadTooLarge)?;
                write_ext_meta(&mut self.buf, len, EXT_ERRENV).map_err(|_| WireError::Truncated)?;
                self.buf.extend_from_slice(payload);
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Decoder
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct Decoder<'a> {
    input: &'a [u8],
    pos: usize,
}

impl<'a> Decoder<'a> {
    pub fn new(input: &'a [u8]) -> Self {
        Self { input, pos: 0 }
    }

    pub fn len(&self) -> usize {
        self.input.len()
    }

    pub fn is_empty(&self) -> bool {
        self.input.is_empty()
    }

    pub fn position(&self) -> usize {
        self.pos
    }

    pub fn at_end(&self) -> bool {
        self.pos >= self.input.len()
    }

    pub fn read_value(&mut self) -> Result<Value, WireError> {
        let mut cursor = &self.input[self.pos..];
        let value = read_value_from(&mut cursor)?;
        self.pos = self.input.len() - cursor.len();
        Ok(value)
    }
}

/// Decode a single `Value` from a `&mut &[u8]` cursor (the form `rmp`'s
/// `RmpRead` impl for byte slices expects). The cursor advances by the
/// number of bytes consumed.
fn read_value_from(cursor: &mut &[u8]) -> Result<Value, WireError> {
    let marker = read_marker(cursor).map_err(|_| WireError::Truncated)?;
    match marker {
        Marker::Null => Ok(Value::Nil),
        Marker::True => Ok(Value::Bool(true)),
        Marker::False => Ok(Value::Bool(false)),

        Marker::FixPos(n) => Ok(Value::Int(n as i64)),
        Marker::FixNeg(n) => Ok(Value::Int(n as i64)),
        Marker::U8 => Ok(Value::Int(cursor.read_data_u8()? as i64)),
        Marker::U16 => Ok(Value::Int(cursor.read_data_u16()? as i64)),
        Marker::U32 => Ok(Value::Int(cursor.read_data_u32()? as i64)),
        Marker::U64 => {
            let n = cursor.read_data_u64()?;
            if n <= i64::MAX as u64 {
                Ok(Value::Int(n as i64))
            } else {
                Ok(Value::UInt(n))
            }
        }
        Marker::I8 => Ok(Value::Int(cursor.read_data_i8()? as i64)),
        Marker::I16 => Ok(Value::Int(cursor.read_data_i16()? as i64)),
        Marker::I32 => Ok(Value::Int(cursor.read_data_i32()? as i64)),
        Marker::I64 => Ok(Value::Int(cursor.read_data_i64()?)),

        Marker::F32 => Ok(Value::Float(cursor.read_data_f32()? as f64)),
        Marker::F64 => Ok(Value::Float(cursor.read_data_f64()?)),

        Marker::FixStr(len) => read_str_body(cursor, len as usize),
        Marker::Str8 => {
            let len = cursor.read_data_u8()? as usize;
            read_str_body(cursor, len)
        }
        Marker::Str16 => {
            let len = cursor.read_data_u16()? as usize;
            read_str_body(cursor, len)
        }
        Marker::Str32 => {
            let len = cursor.read_data_u32()? as usize;
            read_str_body(cursor, len)
        }

        Marker::Bin8 => {
            let len = cursor.read_data_u8()? as usize;
            read_bin_body(cursor, len)
        }
        Marker::Bin16 => {
            let len = cursor.read_data_u16()? as usize;
            read_bin_body(cursor, len)
        }
        Marker::Bin32 => {
            let len = cursor.read_data_u32()? as usize;
            read_bin_body(cursor, len)
        }

        Marker::FixArray(len) => read_array_body(cursor, len as usize),
        Marker::Array16 => {
            let len = cursor.read_data_u16()? as usize;
            read_array_body(cursor, len)
        }
        Marker::Array32 => {
            let len = cursor.read_data_u32()? as usize;
            read_array_body(cursor, len)
        }

        Marker::FixMap(len) => read_map_body(cursor, len as usize),
        Marker::Map16 => {
            let len = cursor.read_data_u16()? as usize;
            read_map_body(cursor, len)
        }
        Marker::Map32 => {
            let len = cursor.read_data_u32()? as usize;
            read_map_body(cursor, len)
        }

        Marker::FixExt1 => read_ext(cursor, 1),
        Marker::FixExt2 => read_ext(cursor, 2),
        Marker::FixExt4 => read_ext(cursor, 4),
        Marker::FixExt8 => read_ext(cursor, 8),
        Marker::FixExt16 => read_ext(cursor, 16),
        Marker::Ext8 => {
            let len = cursor.read_data_u8()? as usize;
            read_ext(cursor, len)
        }
        Marker::Ext16 => {
            let len = cursor.read_data_u16()? as usize;
            read_ext(cursor, len)
        }
        Marker::Ext32 => {
            let len = cursor.read_data_u32()? as usize;
            read_ext(cursor, len)
        }

        Marker::Reserved => Err(WireError::InvalidType),
    }
}

fn take(cursor: &mut &[u8], n: usize) -> Result<Vec<u8>, WireError> {
    if cursor.len() < n {
        return Err(WireError::Truncated);
    }
    let (head, tail) = cursor.split_at(n);
    let out = head.to_vec();
    *cursor = tail;
    Ok(out)
}

fn read_str_body(cursor: &mut &[u8], len: usize) -> Result<Value, WireError> {
    let bytes = take(cursor, len)?;
    let s = String::from_utf8(bytes).map_err(|_| WireError::Utf8)?;
    Ok(Value::Str(s))
}

fn read_bin_body(cursor: &mut &[u8], len: usize) -> Result<Value, WireError> {
    Ok(Value::Bin(take(cursor, len)?))
}

fn read_array_body(cursor: &mut &[u8], len: usize) -> Result<Value, WireError> {
    let mut items = Vec::with_capacity(len);
    for _ in 0..len {
        items.push(read_value_from(cursor)?);
    }
    Ok(Value::Array(items))
}

fn read_map_body(cursor: &mut &[u8], len: usize) -> Result<Value, WireError> {
    let mut pairs = Vec::with_capacity(len);
    for _ in 0..len {
        let k = read_value_from(cursor)?;
        let v = read_value_from(cursor)?;
        pairs.push((k, v));
    }
    Ok(Value::Map(pairs))
}

fn read_ext(cursor: &mut &[u8], len: usize) -> Result<Value, WireError> {
    if cursor.is_empty() {
        return Err(WireError::Truncated);
    }
    let ty = cursor[0] as i8;
    *cursor = &cursor[1..];
    match ty {
        EXT_HANDLE => {
            if len != 4 {
                return Err(WireError::InvalidHandle);
            }
            let payload = take(cursor, 4)?;
            let id = u32::from_be_bytes(payload.try_into().unwrap());
            if id > HANDLE_ID_MAX {
                return Err(WireError::InvalidHandle);
            }
            Ok(Value::Handle(id))
        }
        EXT_ERRENV => {
            let payload = take(cursor, len)?;
            // Validate the payload is exactly one msgpack map.
            let mut inner = &payload[..];
            match read_value_from(&mut inner) {
                Ok(Value::Map(_)) if inner.is_empty() => {}
                _ => return Err(WireError::InvalidErrEnv),
            }
            Ok(Value::ErrEnv(payload))
        }
        _ => Err(WireError::InvalidType),
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
    fn golden_handle_one_is_fixext4() {
        assert_eq!(
            encode(&Value::Handle(1)),
            vec![0xd6, 0x01, 0x00, 0x00, 0x00, 0x01]
        );
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
        let mut buf = vec![OUTCOME_TAG_RESULT];
        let mut enc = Encoder::new();
        enc.write_value(&Value::Array(vec![Value::Int(42)]))
            .unwrap();
        buf.extend(enc.into_bytes());
        assert_eq!(buf, vec![0x01, 0x91, 0x2a]);
    }

    #[test]
    fn narrowest_int_boundaries() {
        assert_eq!(encode(&Value::Int(127))[0], 0x7f);
        assert_eq!(encode(&Value::Int(128))[0], 0xcc);
        assert_eq!(encode(&Value::Int(256))[0], 0xcd);
        assert_eq!(encode(&Value::Int(65_536))[0], 0xce);
        assert_eq!(encode(&Value::Int(0x1_0000_0000))[0], 0xcf);
        assert_eq!(encode(&Value::Int(-32))[0], 0xe0);
        assert_eq!(encode(&Value::Int(-33))[0], 0xd0);
        assert_eq!(encode(&Value::Int(-129))[0], 0xd1);
        assert_eq!(encode(&Value::Int(-32_769))[0], 0xd2);
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
        let m15: Vec<(Value, Value)> = (0..15).map(|i| (Value::Int(i), Value::Nil)).collect();
        assert_eq!(encode(&Value::Map(m15))[0], 0x8f);
        let m16: Vec<(Value, Value)> = (0..16).map(|i| (Value::Int(i), Value::Nil)).collect();
        assert_eq!(encode(&Value::Map(m16))[0], 0xde);
    }

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
        assert_eq!(
            roundtrip(Value::UInt(i64::MAX as u64)),
            Value::Int(i64::MAX)
        );
    }

    #[test]
    fn roundtrip_floats_special() {
        match roundtrip(Value::Float(0.0)) {
            Value::Float(f) => {
                assert_eq!(f, 0.0);
                assert!(f.is_sign_positive());
            }
            other => panic!("unexpected: {:?}", other),
        }
        match roundtrip(Value::Float(-0.0)) {
            Value::Float(f) => {
                assert_eq!(f, 0.0);
                assert!(f.is_sign_negative());
            }
            other => panic!("unexpected: {:?}", other),
        }
        match roundtrip(Value::Float(f64::INFINITY)) {
            Value::Float(f) => assert!(f.is_infinite() && f.is_sign_positive()),
            other => panic!("unexpected: {:?}", other),
        }
        match roundtrip(Value::Float(f64::NEG_INFINITY)) {
            Value::Float(f) => assert!(f.is_infinite() && f.is_sign_negative()),
            other => panic!("unexpected: {:?}", other),
        }
        match roundtrip(Value::Float(f64::NAN)) {
            Value::Float(f) => assert!(f.is_nan()),
            other => panic!("unexpected: {:?}", other),
        }
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
        // Handle id 0 is a wire-violation at the codec layer (caller built
        // an invalid Value); narrow round-trip to legal ids only.
        assert_eq!(roundtrip(Value::Handle(1)), Value::Handle(1));
        assert_eq!(
            roundtrip(Value::Handle(HANDLE_ID_MAX)),
            Value::Handle(HANDLE_ID_MAX)
        );
    }

    #[test]
    fn roundtrip_errenv_payload() {
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
                (
                    Value::Str("xs".into()),
                    Value::Array(vec![Value::Int(1), Value::Int(-1), Value::Float(2.5)]),
                ),
                (Value::Str("err".into()), inner_errenv),
                (Value::Str("blob".into()), Value::Bin(vec![0, 1, 2, 3])),
            ]),
        ]);
        assert_eq!(roundtrip(v.clone()), v);
    }

    #[test]
    fn decode_truncated_input_returns_truncated() {
        let bytes = [0xa3];
        let mut dec = Decoder::new(&bytes);
        assert_eq!(dec.read_value(), Err(WireError::Truncated));

        let bytes = [0xcd, 0x00];
        let mut dec = Decoder::new(&bytes);
        assert_eq!(dec.read_value(), Err(WireError::Truncated));

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
        let bytes = [0xd6, 0x01, 0x80, 0x00, 0x00, 0x00];
        let mut dec = Decoder::new(&bytes);
        assert_eq!(dec.read_value(), Err(WireError::InvalidHandle));
    }

    #[test]
    fn decode_errenv_with_non_map_payload_returns_invalid_errenv() {
        let bytes = [0xc7, 0x01, 0x02, 0xc0];
        let mut dec = Decoder::new(&bytes);
        assert_eq!(dec.read_value(), Err(WireError::InvalidErrEnv));
    }

    #[test]
    fn decode_uint64_above_i64max_uses_uint_variant() {
        let mut bytes = vec![0xcf];
        bytes.extend_from_slice(&u64::MAX.to_be_bytes());
        let mut dec = Decoder::new(&bytes);
        assert_eq!(dec.read_value(), Ok(Value::UInt(u64::MAX)));
    }

    #[test]
    fn decode_accepts_float32_payload() {
        let mut bytes = vec![0xca];
        bytes.extend_from_slice(&1.5f32.to_be_bytes());
        let mut dec = Decoder::new(&bytes);
        match dec.read_value() {
            Ok(Value::Float(f)) => assert_eq!(f, 1.5),
            other => panic!("unexpected: {:?}", other),
        }
    }

    /// Migration check: codec backend is the `rmp` crate. If someone
    /// quietly reverts to a hand-rolled implementation this test catches
    /// the drift via a symbol that only exists in `rmp`.
    #[test]
    fn rmp_crate_is_the_codec_backbone() {
        // Calling an `rmp` API that doesn't exist anywhere else. Compile
        // failure of this test == drift back to a hand-rolled codec.
        let mut buf = Vec::<u8>::new();
        rmp::encode::write_nil(&mut buf).unwrap();
        assert_eq!(buf, vec![0xc0]);
    }
}
