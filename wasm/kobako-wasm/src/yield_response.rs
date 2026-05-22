//! YieldResponse envelope — value object + codec.
//!
//! docs/wire-codec.md § YieldResponse Envelope pins the byte layout:
//! one tag byte (`0x01` ok / `0x02` break / `0x03` reserved-reject /
//! `0x04` error) followed by a single msgpack-encoded payload value.
//! The guest writes one of these into a buffer allocated via
//! `__kobako_alloc` and returns its `(ptr, len)` from
//! `__kobako_yield_to_block`.
//!
//! This module is an independent codec implementation. The host's
//! `lib/kobako/yield.rb` is its symmetric peer; both sides follow SPEC,
//! and round-trip fuzz across the wire is the correctness mechanism.
//! No production caller in S2b — the value object plus encode / decode
//! land here so the wire shape is committed before the export
//! (S4) and the host dispatcher integration (S5+) consume it.

use crate::codec::{CodecError, Decoder, Encoder, Value};
use crate::rpc::envelope::EnvelopeError;

/// First byte of a YieldResponse for the success branch — payload is
/// the block's return value encoded as a single msgpack value.
pub const TAG_OK: u8 = 0x01;
/// First byte for `break val` — payload is the break value.
pub const TAG_BREAK: u8 = 0x02;
/// Reserved for future `return val` support; both sides currently
/// reject this tag as a wire violation (BLOCK_RESEARCH (d)).
pub const TAG_RESERVED: u8 = 0x03;
/// First byte for an error / fault outcome — payload is a
/// `{"class", "message", "backtrace"}` Hash.
pub const TAG_ERROR: u8 = 0x04;

/// docs/wire-codec.md § YieldResponse Envelope. `tag` is one of the
/// three live values (`TAG_OK` / `TAG_BREAK` / `TAG_ERROR`); `value`
/// carries the decoded payload regardless of variant. Variants that
/// reach the value-object layer are always live — `TAG_RESERVED` and
/// any unknown tag are rejected by [`decode_response`] before reaching
/// here.
#[derive(Debug, Clone, PartialEq)]
pub struct Response {
    pub tag: u8,
    pub value: Value,
}

/// Tags that decoders accept on the wire.
const LIVE_TAGS: &[u8] = &[TAG_OK, TAG_BREAK, TAG_ERROR];

/// Encode `response` to YieldResponse bytes: one tag byte followed by
/// an msgpack-encoded `value`.
pub fn encode_response(response: &Response) -> Result<Vec<u8>, EnvelopeError> {
    debug_assert!(
        LIVE_TAGS.contains(&response.tag),
        "Response.tag must be a live tag — caller invariant"
    );
    let mut enc = Encoder::new();
    enc.write_value(&response.value)?;
    let payload = enc.into_bytes();
    let mut out = Vec::with_capacity(1 + payload.len());
    out.push(response.tag);
    out.extend_from_slice(&payload);
    Ok(out)
}

/// Decode `bytes` into a [`Response`]. Rejects empty input, the
/// reserved tag `0x03`, and any tag outside [`LIVE_TAGS`] by returning
/// an [`EnvelopeError::Shape`].
pub fn decode_response(bytes: &[u8]) -> Result<Response, EnvelopeError> {
    let Some((&tag, body)) = bytes.split_first() else {
        return Err(EnvelopeError::Shape(
            "YieldResponse must carry at least one byte",
        ));
    };
    if !LIVE_TAGS.contains(&tag) {
        return Err(EnvelopeError::Shape(match tag {
            TAG_RESERVED => "YieldResponse tag 0x03 is reserved",
            _ => "YieldResponse tag is not recognised",
        }));
    }

    let mut dec = Decoder::new(body);
    let value = dec
        .read_value()
        .map_err(CodecError::from)
        .map_err(EnvelopeError::from)?;
    Ok(Response { tag, value })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_ok_with_primitive() {
        let resp = Response {
            tag: TAG_OK,
            value: Value::Int(42),
        };
        let bytes = encode_response(&resp).unwrap();
        assert_eq!(decode_response(&bytes).unwrap(), resp);
    }

    #[test]
    fn round_trip_break_with_symbol() {
        let resp = Response {
            tag: TAG_BREAK,
            value: Value::Sym("stop".into()),
        };
        let bytes = encode_response(&resp).unwrap();
        assert_eq!(decode_response(&bytes).unwrap(), resp);
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
        let resp = Response {
            tag: TAG_ERROR,
            value: payload,
        };
        let bytes = encode_response(&resp).unwrap();
        assert_eq!(decode_response(&bytes).unwrap(), resp);
    }

    #[test]
    fn decode_rejects_reserved_tag_0x03() {
        // Tag 0x03 followed by msgpack nil.
        let mut bytes = vec![TAG_RESERVED];
        let mut enc = Encoder::new();
        enc.write_value(&Value::Nil).unwrap();
        bytes.extend_from_slice(&enc.into_bytes());
        let err = decode_response(&bytes).unwrap_err();
        match err {
            EnvelopeError::Shape(msg) => assert!(msg.contains("reserved")),
            other => panic!("expected EnvelopeError::Shape, got {other:?}"),
        }
    }

    #[test]
    fn decode_rejects_unknown_tag() {
        let mut bytes = vec![0x7e];
        let mut enc = Encoder::new();
        enc.write_value(&Value::Nil).unwrap();
        bytes.extend_from_slice(&enc.into_bytes());
        assert!(matches!(
            decode_response(&bytes),
            Err(EnvelopeError::Shape(_))
        ));
    }

    #[test]
    fn decode_rejects_empty_bytes() {
        assert!(matches!(decode_response(&[]), Err(EnvelopeError::Shape(_))));
    }

    #[test]
    fn encode_ok_with_int_42_golden() {
        // Same byte sequence as the Ruby golden test in
        // test_yield_response.rb: tag 0x01 + msgpack int 42 (0x2a).
        let bytes = encode_response(&Response {
            tag: TAG_OK,
            value: Value::Int(42),
        })
        .unwrap();
        assert_eq!(bytes, vec![0x01, 0x2a]);
    }

    #[test]
    fn encode_break_with_nil_golden() {
        // Tag 0x02 + msgpack nil (0xc0).
        let bytes = encode_response(&Response {
            tag: TAG_BREAK,
            value: Value::Nil,
        })
        .unwrap();
        assert_eq!(bytes, vec![0x02, 0xc0]);
    }
}
