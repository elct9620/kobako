//! Encoder: a `Value` tree to its kobako-codec wire bytes, plus the
//! `Encode` trait every wire value object implements.

use rmp::encode::{
    write_array_len, write_bin, write_bool, write_ext_meta, write_f64, write_map_len, write_nil,
    write_sint, write_str, write_uint,
};

use super::{Error, Value, EXT_ERRENV, EXT_HANDLE, EXT_SYMBOL, HANDLE_ID_MAX, MAX_NESTING_DEPTH};

#[derive(Debug, Default)]
pub struct Encoder {
    buf: Vec<u8>,
}

impl Encoder {
    pub fn new() -> Self {
        Self { buf: Vec::new() }
    }

    /// Encode one `Value` to its wire bytes — the single-value entry the
    /// transport envelopes share, peer of the Ruby codec's `Encoder.encode`.
    pub fn encode(value: &Value) -> Result<Vec<u8>, Error> {
        let mut enc = Encoder::new();
        enc.write_value(value)?;
        Ok(enc.into_bytes())
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.buf
    }

    pub fn write_value(&mut self, value: &Value) -> Result<(), Error> {
        self.write_value_at(value, 0)
    }

    /// Encode one `Value` at nesting level `depth`. Recursing past
    /// `MAX_NESTING_DEPTH` is refused as a clean error, mirroring the
    /// decoder's `read_value_from` guard so an over-deep `Value` tree cannot
    /// overflow the wasm stack on the way out either.
    fn write_value_at(&mut self, value: &Value, depth: usize) -> Result<(), Error> {
        if depth > MAX_NESTING_DEPTH {
            return Err(Error::Malformed("nesting exceeds maximum depth"));
        }
        match value {
            Value::Nil => write_nil(&mut self.buf).map_err(|_| Error::Truncated)?,
            Value::Bool(b) => write_bool(&mut self.buf, *b).map_err(|_| Error::Truncated)?,
            Value::Int(n) => {
                write_sint(&mut self.buf, *n).map_err(|_| Error::Truncated)?;
            }
            Value::UInt(n) => {
                write_uint(&mut self.buf, *n).map_err(|_| Error::Truncated)?;
            }
            Value::Float(f) => {
                write_f64(&mut self.buf, *f).map_err(|_| Error::Truncated)?;
            }
            Value::Str(s) => write_str(&mut self.buf, s).map_err(|_| Error::Truncated)?,
            Value::Bin(b) => write_bin(&mut self.buf, b).map_err(|_| Error::Truncated)?,
            Value::Array(items) => {
                let len = u32::try_from(items.len()).map_err(|_| Error::PayloadTooLarge)?;
                write_array_len(&mut self.buf, len).map_err(|_| Error::Truncated)?;
                for item in items {
                    self.write_value_at(item, depth + 1)?;
                }
            }
            Value::Map(pairs) => {
                let len = u32::try_from(pairs.len()).map_err(|_| Error::PayloadTooLarge)?;
                write_map_len(&mut self.buf, len).map_err(|_| Error::Truncated)?;
                for (k, v) in pairs {
                    self.write_value_at(k, depth + 1)?;
                    self.write_value_at(v, depth + 1)?;
                }
            }
            Value::Sym(name) => {
                let bytes = name.as_bytes();
                let len = u32::try_from(bytes.len()).map_err(|_| Error::PayloadTooLarge)?;
                write_ext_meta(&mut self.buf, len, EXT_SYMBOL).map_err(|_| Error::Truncated)?;
                self.buf.extend_from_slice(bytes);
            }
            Value::Handle(id) => {
                if *id == 0 || *id > HANDLE_ID_MAX {
                    return Err(Error::InvalidHandle);
                }
                write_ext_meta(&mut self.buf, 4, EXT_HANDLE).map_err(|_| Error::Truncated)?;
                self.buf.extend_from_slice(&id.to_be_bytes());
            }
            Value::ErrEnv(payload) => {
                let len = u32::try_from(payload.len()).map_err(|_| Error::PayloadTooLarge)?;
                write_ext_meta(&mut self.buf, len, EXT_ERRENV).map_err(|_| Error::Truncated)?;
                self.buf.extend_from_slice(payload);
            }
        }
        Ok(())
    }
}

/// A wire value object that encodes itself to its kobako-codec bytes.
/// Implemented by every envelope that crosses the Transport wire — the
/// per-call envelopes (`transport::{Request, Response, Yield}`) and the
/// per-run `Outcome` / `Panic` records alike — which is why the trait
/// lives here at the codec tier rather than under `transport`. It is the
/// Rust-native expression of the contract the Ruby host gets via duck
/// typing (`#encode` on each value object). The value object's own
/// invariants are the contract; this does not re-validate the shape.
/// Faults surface as `Error` — the same type the byte-level codec
/// raises — so a value object is encoded as a whole through one error
/// channel.
pub trait Encode {
    fn encode(&self) -> Result<Vec<u8>, Error>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode(v: &Value) -> Vec<u8> {
        Encoder::encode(v).expect("encode")
    }

    #[test]
    fn encoder_starts_empty() {
        let enc = Encoder::new();
        assert!(enc.into_bytes().is_empty());
    }

    #[test]
    fn ext_codes_match_spec() {
        assert_eq!(EXT_SYMBOL, 0x00);
        assert_eq!(EXT_HANDLE, 0x01);
        assert_eq!(EXT_ERRENV, 0x02);
    }

    #[test]
    fn handle_id_cap_matches_spec() {
        assert_eq!(HANDLE_ID_MAX, (1u32 << 31) - 1);
    }

    #[test]
    fn encoder_accepts_nesting_at_max_depth() {
        // The encode-side guard mirrors the decode-side boundary: a value
        // nested exactly to the cap must encode, so the two paths agree on
        // which structures are legal (docs/wire-codec.md § Structural
        // Nesting Depth).
        let mut v = Value::Nil;
        for _ in 0..MAX_NESTING_DEPTH {
            v = Value::Array(vec![v]);
        }
        let mut enc = Encoder::new();
        assert!(
            enc.write_value(&v).is_ok(),
            "a value nested to the cap must encode, matching the decoder's accepted boundary"
        );
    }

    #[test]
    fn encoder_rejects_nesting_past_max_depth() {
        // One level past the cap fails as a clean wire error instead of
        // recursing until the wasm stack overflows and hard-traps the guest
        // — the encode-side twin of decoder_rejects_nesting_past_max_depth.
        let mut v = Value::Nil;
        for _ in 0..(MAX_NESTING_DEPTH + 1) {
            v = Value::Array(vec![v]);
        }
        let mut enc = Encoder::new();
        assert_eq!(
            enc.write_value(&v),
            Err(Error::Malformed("nesting exceeds maximum depth"))
        );
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
    fn golden_sym_empty_uses_ext8_with_zero_length() {
        // docs/wire-codec.md § Ext Types → ext 0x00: `c7 00 00` is the empty Symbol.
        assert_eq!(encode(&Value::Sym(String::new())), vec![0xc7, 0x00, 0x00]);
    }

    #[test]
    fn golden_sym_5byte_uses_ext8() {
        let bytes = encode(&Value::Sym("hello".into()));
        // `c7 05 00 'h' 'e' 'l' 'l' 'o'`
        assert_eq!(bytes, vec![0xc7, 0x05, 0x00, b'h', b'e', b'l', b'l', b'o']);
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
    fn encode_handle_outside_id_range_returns_invalid_handle() {
        // The encoder refuses to emit what the decoder would reject, so
        // an invalid Handle value fails at its origin, not at the peer.
        assert_eq!(
            Encoder::encode(&Value::Handle(0)),
            Err(Error::InvalidHandle)
        );
        assert_eq!(
            Encoder::encode(&Value::Handle(HANDLE_ID_MAX + 1)),
            Err(Error::InvalidHandle)
        );
    }
}
