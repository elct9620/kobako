//! Decoder: kobako-codec wire bytes back to a `Value` tree, plus the
//! `Decode` trait every wire value object implements.

use rmp::decode::{read_marker, RmpRead};
use rmp::Marker;

use super::{Error, Value, EXT_ERRENV, EXT_HANDLE, EXT_SYMBOL, HANDLE_ID_MAX, MAX_NESTING_DEPTH};

#[derive(Debug)]
pub struct Decoder<'a> {
    input: &'a [u8],
    pos: usize,
}

impl<'a> Decoder<'a> {
    pub fn new(input: &'a [u8]) -> Self {
        Self { input, pos: 0 }
    }

    pub fn position(&self) -> usize {
        self.pos
    }

    pub fn at_end(&self) -> bool {
        self.pos >= self.input.len()
    }

    pub fn read_value(&mut self) -> Result<Value, Error> {
        let mut cursor = &self.input[self.pos..];
        let value = read_value_from(&mut cursor, 0)?;
        self.pos = self.input.len() - cursor.len();
        Ok(value)
    }

    /// Read a single value and require it to consume the whole input. Every
    /// kobako envelope is exactly one msgpack value, so bytes left over
    /// after it signal a host↔guest framing desync. The host codec rejects
    /// the same case ("extra bytes after the deserialized object"); matching
    /// that here keeps the two wire peers equally strict and fails loud
    /// instead of silently decoding a truncated stream.
    pub fn read_only_value(&mut self) -> Result<Value, Error> {
        let value = self.read_value()?;
        if !self.at_end() {
            return Err(Error::Malformed("trailing bytes after the envelope value"));
        }
        Ok(value)
    }
}

/// Decode a single `Value` from a `&mut &[u8]` cursor (the form `rmp`'s
/// `RmpRead` impl for byte slices expects). The cursor advances by the
/// number of bytes consumed. `depth` is the current nesting level;
/// recursing past `MAX_NESTING_DEPTH` is refused as a clean error so a
/// deeply nested payload cannot overflow the wasm stack.
fn read_value_from(cursor: &mut &[u8], depth: usize) -> Result<Value, Error> {
    if depth > MAX_NESTING_DEPTH {
        return Err(Error::Malformed("nesting exceeds maximum depth"));
    }
    let marker = read_marker(cursor).map_err(|_| Error::Truncated)?;
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

        Marker::FixArray(len) => read_array_body(cursor, len as usize, depth),
        Marker::Array16 => {
            let len = cursor.read_data_u16()? as usize;
            read_array_body(cursor, len, depth)
        }
        Marker::Array32 => {
            let len = cursor.read_data_u32()? as usize;
            read_array_body(cursor, len, depth)
        }

        Marker::FixMap(len) => read_map_body(cursor, len as usize, depth),
        Marker::Map16 => {
            let len = cursor.read_data_u16()? as usize;
            read_map_body(cursor, len, depth)
        }
        Marker::Map32 => {
            let len = cursor.read_data_u32()? as usize;
            read_map_body(cursor, len, depth)
        }

        Marker::FixExt1 => read_ext(cursor, 1, depth),
        Marker::FixExt2 => read_ext(cursor, 2, depth),
        Marker::FixExt4 => read_ext(cursor, 4, depth),
        Marker::FixExt8 => read_ext(cursor, 8, depth),
        Marker::FixExt16 => read_ext(cursor, 16, depth),
        Marker::Ext8 => {
            let len = cursor.read_data_u8()? as usize;
            read_ext(cursor, len, depth)
        }
        Marker::Ext16 => {
            let len = cursor.read_data_u16()? as usize;
            read_ext(cursor, len, depth)
        }
        Marker::Ext32 => {
            let len = cursor.read_data_u32()? as usize;
            read_ext(cursor, len, depth)
        }

        Marker::Reserved => Err(Error::InvalidType),
    }
}

fn take(cursor: &mut &[u8], n: usize) -> Result<Vec<u8>, Error> {
    if cursor.len() < n {
        return Err(Error::Truncated);
    }
    let (head, tail) = cursor.split_at(n);
    let out = head.to_vec();
    *cursor = tail;
    Ok(out)
}

fn read_str_body(cursor: &mut &[u8], len: usize) -> Result<Value, Error> {
    let bytes = take(cursor, len)?;
    let s = String::from_utf8(bytes).map_err(|_| Error::Utf8)?;
    Ok(Value::Str(s))
}

fn read_bin_body(cursor: &mut &[u8], len: usize) -> Result<Value, Error> {
    Ok(Value::Bin(take(cursor, len)?))
}

fn read_array_body(cursor: &mut &[u8], len: usize, depth: usize) -> Result<Value, Error> {
    // Every msgpack value is at least one byte, so the element count
    // cannot exceed the bytes remaining; cap the pre-allocation at that
    // bound so a forged `array 32` length cannot trigger a huge eager
    // allocation before the read loop reaches the clean `Truncated`.
    let mut items = Vec::with_capacity(len.min(cursor.len()));
    for _ in 0..len {
        items.push(read_value_from(cursor, depth + 1)?);
    }
    Ok(Value::Array(items))
}

fn read_map_body(cursor: &mut &[u8], len: usize, depth: usize) -> Result<Value, Error> {
    // Same bound as `read_array_body`: a pair is at least two bytes, so
    // `cursor.len()` is a safe upper bound on the pair count.
    let mut pairs = Vec::with_capacity(len.min(cursor.len()));
    for _ in 0..len {
        let k = read_value_from(cursor, depth + 1)?;
        let v = read_value_from(cursor, depth + 1)?;
        pairs.push((k, v));
    }
    Ok(Value::Map(pairs))
}

fn read_ext(cursor: &mut &[u8], len: usize, depth: usize) -> Result<Value, Error> {
    if cursor.is_empty() {
        return Err(Error::Truncated);
    }
    let ty = cursor[0] as i8;
    *cursor = &cursor[1..];
    match ty {
        EXT_SYMBOL => {
            let payload = take(cursor, len)?;
            let name = String::from_utf8(payload).map_err(|_| Error::Utf8)?;
            Ok(Value::Sym(name))
        }
        EXT_HANDLE => {
            if len != 4 {
                return Err(Error::InvalidHandle);
            }
            let payload = take(cursor, 4)?;
            let id = u32::from_be_bytes(payload.try_into().unwrap());
            if id == 0 || id > HANDLE_ID_MAX {
                return Err(Error::InvalidHandle);
            }
            Ok(Value::Handle(id))
        }
        EXT_ERRENV => {
            let payload = take(cursor, len)?;
            // Validate the payload is exactly one msgpack map.
            let mut inner = &payload[..];
            match read_value_from(&mut inner, depth + 1) {
                Ok(Value::Map(_)) if inner.is_empty() => {}
                _ => return Err(Error::InvalidErrEnv),
            }
            Ok(Value::ErrEnv(payload))
        }
        _ => Err(Error::InvalidType),
    }
}

/// The decode half of `Encode`: rebuild a wire value object from its
/// kobako-codec bytes. Returns `Error::Malformed` when the bytes parse
/// as a value but do not match the expected envelope shape. Types that
/// only travel one direction (e.g. the host→guest invocation envelope)
/// implement only the half they need.
pub trait Decode: Sized {
    fn decode(bytes: &[u8]) -> Result<Self, Error>;
}

#[cfg(test)]
mod tests {
    use super::*;
    // The round-trip cases exercise the decoder's reconstruction of what
    // the encoder wrote, so the sibling `Encoder` rides in as a fixture.
    use crate::codec::Encoder;

    fn roundtrip(v: Value) -> Value {
        let bytes = encode(&v);
        let mut dec = Decoder::new(&bytes);
        let out = dec.read_value().expect("decode");
        assert!(dec.at_end(), "decoder must consume all bytes");
        out
    }

    fn encode(v: &Value) -> Vec<u8> {
        Encoder::encode(v).expect("encode")
    }

    #[test]
    fn decoder_tracks_position() {
        let bytes = [0xc0_u8];
        let dec = Decoder::new(&bytes);
        assert_eq!(dec.position(), 0);
        assert!(!dec.at_end());
    }

    #[test]
    fn decoder_empty_input_is_at_end() {
        let dec = Decoder::new(&[]);
        assert!(dec.at_end());
    }

    #[test]
    fn read_only_value_accepts_a_sole_value() {
        let bytes = encode(&Value::Int(42));
        let mut dec = Decoder::new(&bytes);
        assert_eq!(dec.read_only_value(), Ok(Value::Int(42)));
    }

    #[test]
    fn read_only_value_rejects_trailing_bytes() {
        // Two concatenated values: an envelope buffer must hold exactly
        // one, so the second is a framing desync the host codec rejects too.
        let mut bytes = encode(&Value::Int(1));
        bytes.extend(encode(&Value::Int(2)));
        let mut dec = Decoder::new(&bytes);
        assert_eq!(
            dec.read_only_value(),
            Err(Error::Malformed("trailing bytes after the envelope value"))
        );
    }

    #[test]
    fn decoder_accepts_nesting_at_max_depth() {
        // A value nested exactly to the cap round-trips — the boundary
        // value is accepted, mirroring the encode-side limit
        // (docs/wire-codec.md § Structural Nesting Depth).
        let mut v = Value::Nil;
        for _ in 0..MAX_NESTING_DEPTH {
            v = Value::Array(vec![v]);
        }
        assert_eq!(roundtrip(v.clone()), v);
    }

    #[test]
    fn decoder_rejects_nesting_past_max_depth() {
        // One level past the cap fails as a clean wire error instead of
        // recursing until the wasm stack overflows and hard-traps the
        // guest (docs/wire-codec.md § Structural Nesting Depth). The bytes
        // are hand-built — `0x91` is a one-element fixarray, `0xc0` the
        // innermost nil — because the encoder now refuses to emit an
        // over-deep tree (encoder_rejects_nesting_past_max_depth), so it
        // cannot serialize this input for us.
        let mut bytes = vec![0x91u8; MAX_NESTING_DEPTH + 1];
        bytes.push(0xc0);
        let mut dec = Decoder::new(&bytes);
        assert_eq!(
            dec.read_value(),
            Err(Error::Malformed("nesting exceeds maximum depth"))
        );
    }

    #[test]
    fn decoder_rejects_forged_array_length_without_eager_alloc() {
        // An `array 32` header claiming u32::MAX elements with no body
        // must fail as a clean Truncated error — not pre-allocate a
        // multi-gigabyte Vec and abort before the read loop runs. The
        // element count cannot exceed the remaining bytes.
        let bytes = [0xdd, 0xff, 0xff, 0xff, 0xff];
        let mut dec = Decoder::new(&bytes);
        assert_eq!(dec.read_value(), Err(Error::Truncated));
    }

    #[test]
    fn decoder_rejects_forged_map_length_without_eager_alloc() {
        // Same as the array case for a `map 32` header claiming
        // u32::MAX pairs with no body.
        let bytes = [0xdf, 0xff, 0xff, 0xff, 0xff];
        let mut dec = Decoder::new(&bytes);
        assert_eq!(dec.read_value(), Err(Error::Truncated));
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
        // 16 entries cross the fixmap ceiling into a map 16 frame, so the
        // decoder's wider length arm runs (mirrors the array 16 case above).
        let big = Value::Map((0..16).map(|i| (Value::Int(i), Value::Nil)).collect());
        assert_eq!(roundtrip(big.clone()), big);
    }

    #[test]
    fn roundtrip_sym_payload_sizes() {
        // Empty Symbol (`:""`) is wire-legal — exercised explicitly so a
        // future encoder regression that emits no-payload framing fails.
        // The sweep spans every ext frame width so each decode arm runs:
        // 16 bytes rides a fixext16 frame, 65536 an ext 32 frame.
        for name in [
            String::new(),
            "a".to_string(),
            "ab".to_string(),
            "abc".to_string(),
            "abcdefgh".to_string(),
            "a".repeat(16),
            "a".repeat(255),
            "a".repeat(256),
            "a".repeat(65_536),
            "蒼時弦也".to_string(),
        ] {
            assert_eq!(roundtrip(Value::Sym(name.clone())), Value::Sym(name));
        }
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
    fn decode_sym_with_invalid_utf8_returns_utf8_error() {
        // `c7 02 00 ff fe` — ext 8, len=2, type=0x00, non-UTF-8 bytes.
        let bytes = [0xc7, 0x02, 0x00, 0xff, 0xfe];
        let mut dec = Decoder::new(&bytes);
        assert_eq!(dec.read_value(), Err(Error::Utf8));
    }

    #[test]
    fn decode_truncated_input_returns_truncated() {
        let bytes = [0xa3];
        let mut dec = Decoder::new(&bytes);
        assert_eq!(dec.read_value(), Err(Error::Truncated));

        let bytes = [0xcd, 0x00];
        let mut dec = Decoder::new(&bytes);
        assert_eq!(dec.read_value(), Err(Error::Truncated));

        let mut dec = Decoder::new(&[]);
        assert_eq!(dec.read_value(), Err(Error::Truncated));

        // fixext 1 marker with its type byte truncated — read_ext meets an
        // empty cursor where the ext type must be.
        let mut dec = Decoder::new(&[0xd4]);
        assert_eq!(dec.read_value(), Err(Error::Truncated));
    }

    #[test]
    fn decode_invalid_type_tag_returns_invalid_type() {
        // 0xc1 is reserved/never used in msgpack.
        let bytes = [0xc1];
        let mut dec = Decoder::new(&bytes);
        assert_eq!(dec.read_value(), Err(Error::InvalidType));

        // fixext 1 with an unknown ext code (0x05 — not 0x01 or 0x02).
        let bytes = [0xd4, 0x05, 0x00];
        let mut dec = Decoder::new(&bytes);
        assert_eq!(dec.read_value(), Err(Error::InvalidType));
    }

    #[test]
    fn decode_invalid_utf8_in_str_returns_utf8() {
        let bytes = [0xa2, 0xff, 0xfe];
        let mut dec = Decoder::new(&bytes);
        assert_eq!(dec.read_value(), Err(Error::Utf8));
    }

    #[test]
    fn decode_handle_with_wrong_payload_length_returns_invalid_handle() {
        // fixext 1 with type 0x01 — Handle with only 1 payload byte.
        let bytes = [0xd4, 0x01, 0x00];
        let mut dec = Decoder::new(&bytes);
        assert_eq!(dec.read_value(), Err(Error::InvalidHandle));
    }

    #[test]
    fn decode_handle_above_cap_returns_invalid_handle() {
        let bytes = [0xd6, 0x01, 0x80, 0x00, 0x00, 0x00];
        let mut dec = Decoder::new(&bytes);
        assert_eq!(dec.read_value(), Err(Error::InvalidHandle));
    }

    #[test]
    fn decode_handle_zero_returns_invalid_handle() {
        // ID 0 is the reserved invalid sentinel (docs/wire-codec.md
        // § ext 0x01); forged bytes carrying it must be a wire violation,
        // matching the Ruby peer's Handle::MIN_ID floor.
        let bytes = [0xd6, 0x01, 0x00, 0x00, 0x00, 0x00];
        let mut dec = Decoder::new(&bytes);
        assert_eq!(dec.read_value(), Err(Error::InvalidHandle));
    }

    #[test]
    fn decode_errenv_with_non_map_payload_returns_invalid_errenv() {
        let bytes = [0xc7, 0x01, 0x02, 0xc0];
        let mut dec = Decoder::new(&bytes);
        assert_eq!(dec.read_value(), Err(Error::InvalidErrEnv));
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
}
