//! YieldResponse envelope — the `Yield` value object + its wire codec.
//!
//! docs/wire-codec.md § YieldResponse Envelope pins the byte layout:
//! one tag byte (`0x01` ok / `0x02` break / `0x03` reserved-reject /
//! `0x04` error) followed by a single msgpack-encoded payload value.
//! The guest writes one of these into a buffer allocated via
//! `__kobako_alloc` and returns its `(ptr, len)` from
//! `__kobako_yield_to_block`.
//!
//! This module is an independent codec implementation. The host's
//! `lib/kobako/transport/yield.rb` is its symmetric peer; both sides
//! follow SPEC, and round-trip fuzz across the wire is the correctness
//! mechanism.

use crate::codec::{self, Decoder, Encoder, Value};
use crate::transport::{Decode, Encode};

/// First byte of a YieldResponse for the success branch — payload is
/// the block's return value encoded as a single msgpack value.
pub const TAG_OK: u8 = 0x01;
/// First byte for `break val` — payload is the break value.
pub const TAG_BREAK: u8 = 0x02;
/// Reserved for future `return val` support; both sides reject this
/// tag as a wire violation (YieldResponse envelope contract).
pub const TAG_RESERVED: u8 = 0x03;
/// First byte for an error / fault outcome — payload is a
/// `{"class", "message", "backtrace"}` Hash.
pub const TAG_ERROR: u8 = 0x04;

/// docs/wire-codec.md § YieldResponse Envelope. `tag` is one of the
/// three live values (`TAG_OK` / `TAG_BREAK` / `TAG_ERROR`); `value`
/// carries the decoded payload regardless of variant. Variants that
/// reach the value-object layer are always live — `TAG_RESERVED` and
/// any unknown tag are rejected by [`Yield::decode`] before reaching here.
#[derive(Debug, Clone, PartialEq)]
pub struct Yield {
    pub tag: u8,
    pub value: Value,
}

/// Tags that decoders accept on the wire.
const LIVE_TAGS: &[u8] = &[TAG_OK, TAG_BREAK, TAG_ERROR];

impl Encode for Yield {
    /// Encode to YieldResponse bytes: one tag byte followed by an
    /// msgpack-encoded `value`.
    fn encode(&self) -> Result<Vec<u8>, codec::Error> {
        debug_assert!(
            LIVE_TAGS.contains(&self.tag),
            "Yield.tag must be a live tag — caller invariant"
        );
        let mut enc = Encoder::new();
        enc.write_value(&self.value)?;
        let payload = enc.into_bytes();
        let mut out = Vec::with_capacity(1 + payload.len());
        out.push(self.tag);
        out.extend_from_slice(&payload);
        Ok(out)
    }
}

impl Decode for Yield {
    /// Decode `bytes` into a [`Yield`]. Rejects empty input, the reserved
    /// tag `0x03`, and any tag outside [`LIVE_TAGS`] by returning
    /// [`codec::Error::Malformed`].
    fn decode(bytes: &[u8]) -> Result<Self, codec::Error> {
        let Some((&tag, body)) = bytes.split_first() else {
            return Err(codec::Error::Malformed(
                "YieldResponse must carry at least one byte",
            ));
        };
        if !LIVE_TAGS.contains(&tag) {
            return Err(codec::Error::Malformed(match tag {
                TAG_RESERVED => "YieldResponse tag 0x03 is reserved",
                _ => "YieldResponse tag is not recognised",
            }));
        }

        let mut dec = Decoder::new(body);
        let value = dec.read_value()?;
        Ok(Yield { tag, value })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_ok_with_primitive() {
        let resp = Yield {
            tag: TAG_OK,
            value: Value::Int(42),
        };
        let bytes = resp.encode().unwrap();
        assert_eq!(Yield::decode(&bytes).unwrap(), resp);
    }

    #[test]
    fn round_trip_break_with_symbol() {
        let resp = Yield {
            tag: TAG_BREAK,
            value: Value::Sym("stop".into()),
        };
        let bytes = resp.encode().unwrap();
        assert_eq!(Yield::decode(&bytes).unwrap(), resp);
    }

    #[test]
    fn round_trip_error_with_class_message_backtrace_map() {
        let payload = Value::Map(vec![
            (
                Value::Str("class".into()),
                Value::Str("RuntimeError".into()),
            ),
            (Value::Str("message".into()), Value::Str("boom".into())),
            (
                Value::Str("backtrace".into()),
                Value::Array(vec![Value::Str("(eval):1:in `block'".into())]),
            ),
        ]);
        let resp = Yield {
            tag: TAG_ERROR,
            value: payload,
        };
        let bytes = resp.encode().unwrap();
        assert_eq!(Yield::decode(&bytes).unwrap(), resp);
    }

    #[test]
    fn decode_rejects_reserved_tag_0x03() {
        // Tag 0x03 followed by msgpack nil.
        let mut bytes = vec![TAG_RESERVED];
        let mut enc = Encoder::new();
        enc.write_value(&Value::Nil).unwrap();
        bytes.extend_from_slice(&enc.into_bytes());
        let err = Yield::decode(&bytes).unwrap_err();
        match err {
            codec::Error::Malformed(msg) => assert!(msg.contains("reserved")),
            other => panic!("expected codec::Error::Malformed, got {other:?}"),
        }
    }

    #[test]
    fn decode_rejects_unknown_tag() {
        let mut bytes = vec![0x7e];
        let mut enc = Encoder::new();
        enc.write_value(&Value::Nil).unwrap();
        bytes.extend_from_slice(&enc.into_bytes());
        assert!(matches!(
            Yield::decode(&bytes),
            Err(codec::Error::Malformed(_))
        ));
    }

    #[test]
    fn decode_rejects_empty_bytes() {
        assert!(matches!(
            Yield::decode(&[]),
            Err(codec::Error::Malformed(_))
        ));
    }

    #[test]
    fn encode_ok_with_int_42_golden() {
        // Same byte sequence as the Ruby golden test in
        // test_yield_response.rb: tag 0x01 + msgpack int 42 (0x2a).
        let bytes = Yield {
            tag: TAG_OK,
            value: Value::Int(42),
        }
        .encode()
        .unwrap();
        assert_eq!(bytes, vec![0x01, 0x2a]);
    }

    #[test]
    fn encode_break_with_nil_golden() {
        // Tag 0x02 + msgpack nil (0xc0).
        let bytes = Yield {
            tag: TAG_BREAK,
            value: Value::Nil,
        }
        .encode()
        .unwrap();
        assert_eq!(bytes, vec![0x02, 0xc0]);
    }
}
