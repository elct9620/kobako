//! Response envelope — the host→guest reply value object + its wire codec.
//!
//! docs/wire-codec.md § Envelope Encoding → Response pins the binary
//! framing; this assembles it on top of the lower-level [`Encoder`] /
//! [`Decoder`] primitives in `codec/`. Mirrors the host's
//! `lib/kobako/transport/response.rb`.
//!
//! No `unsafe`. No third-party dependencies. An independent
//! re-implementation of SPEC; byte-compatible with the host because both
//! follow SPEC, not because one was copied from the other.

use crate::codec::{self, Decoder, Encoder, Value};

/// Response variant marker for the success branch
/// (docs/wire-codec.md § Envelope Encoding → Response). Module-private — `Response::Ok`
/// / `Response::Err` are the public surface and reify these values.
const STATUS_OK: i64 = 0;
/// Response variant marker for the error branch. Module-private.
const STATUS_ERROR: i64 = 1;

/// docs/wire-codec.md § Envelope Encoding → Response: 2-element msgpack
/// array `[status, value-or-error-envelope]`. The two variants are
/// mutually exclusive.
#[derive(Debug, Clone, PartialEq)]
pub enum Response {
    /// Success: `status=0`, `value` carries the return value.
    Ok(Value),
    /// Error: `status=1`, payload is a SPEC ext 0x02 Fault envelope
    /// (we keep it as the raw payload bytes, matching `Value::ErrEnv`).
    Err(Vec<u8>),
}

impl codec::Encode for Response {
    fn encode(&self) -> Result<Vec<u8>, codec::Error> {
        let (status, payload) = match self {
            Response::Ok(v) => (STATUS_OK, v.clone()),
            Response::Err(payload_bytes) => (STATUS_ERROR, Value::ErrEnv(payload_bytes.clone())),
        };
        let frame = Value::Array(vec![Value::Int(status), payload]);
        let mut enc = Encoder::new();
        enc.write_value(&frame)?;
        Ok(enc.into_bytes())
    }
}

impl codec::Decode for Response {
    fn decode(bytes: &[u8]) -> Result<Self, codec::Error> {
        let mut dec = Decoder::new(bytes);
        let frame = dec.read_value()?;
        let [status, payload]: [Value; 2] = match frame {
            Value::Array(items) if items.len() == 2 => items.try_into().unwrap(),
            _ => {
                return Err(codec::Error::Malformed(
                    "Response must be a 2-element array",
                ))
            }
        };
        let status = match status {
            Value::Int(n) => n,
            _ => return Err(codec::Error::Malformed("Response status must be int")),
        };
        match status {
            STATUS_OK => Ok(Response::Ok(payload)),
            STATUS_ERROR => match payload {
                Value::ErrEnv(bytes) => Ok(Response::Err(bytes)),
                _ => Err(codec::Error::Malformed(
                    "Response status=1 payload must be ext 0x02 Fault",
                )),
            },
            _ => Err(codec::Error::Malformed("Response status must be 0 or 1")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{Decode, Encode};

    fn errenv_payload(typ: &str, message: &str) -> Vec<u8> {
        // Build a minimal valid ext 0x02 inner map: {type, message, details=nil}.
        let mut enc = Encoder::new();
        enc.write_value(&Value::Map(vec![
            (Value::Str("type".into()), Value::Str(typ.into())),
            (Value::Str("message".into()), Value::Str(message.into())),
            (Value::Str("details".into()), Value::Nil),
        ]))
        .unwrap();
        enc.into_bytes()
    }

    #[test]
    fn response_ok_round_trip_with_primitive() {
        let resp = Response::Ok(Value::Int(42));
        let out = Response::decode(&resp.encode().unwrap()).unwrap();
        assert_eq!(resp, out);
    }

    #[test]
    fn response_ok_round_trip_with_handle() {
        let resp = Response::Ok(Value::Handle(99));
        let out = Response::decode(&resp.encode().unwrap()).unwrap();
        assert_eq!(resp, out);
    }

    #[test]
    fn response_err_round_trip() {
        let payload = errenv_payload("runtime", "boom");
        let resp = Response::Err(payload);
        let out = Response::decode(&resp.encode().unwrap()).unwrap();
        assert_eq!(resp, out);
    }

    #[test]
    fn response_decode_rejects_status_two() {
        let mut enc = Encoder::new();
        enc.write_value(&Value::Array(vec![Value::Int(2), Value::Nil]))
            .unwrap();
        assert!(matches!(
            Response::decode(&enc.into_bytes()),
            Err(codec::Error::Malformed(_))
        ));
    }

    #[test]
    fn response_decode_err_requires_errenv_payload() {
        let mut enc = Encoder::new();
        enc.write_value(&Value::Array(vec![
            Value::Int(1),
            Value::Str("oops".into()),
        ]))
        .unwrap();
        assert!(matches!(
            Response::decode(&enc.into_bytes()),
            Err(codec::Error::Malformed(_))
        ));
    }

    #[test]
    fn response_ok_golden_for_42() {
        let bytes = Response::Ok(Value::Int(42)).encode().unwrap();
        assert_eq!(bytes, vec![0x92, 0x00, 0x2a]);
    }
}
