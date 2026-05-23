//! Per-call RPC envelope encoders/decoders.
//!
//! docs/wire-contract.md pins the logical shape of every host↔guest
//! Request / Response; docs/wire-codec.md § Envelope Encoding
//! pins the binary framing. This module assembles the per-RPC Request
//! and Response envelopes on top of the lower-level [`Encoder`] /
//! [`Decoder`] primitives in `codec/`. The per-`#run` Outcome envelope
//! (Result / Panic) lives next door at `crate::outcome` — mirroring the
//! host's `lib/kobako/transport/{request,response}.rb` vs
//! `lib/kobako/outcome.rb` split.
//!
//! No `unsafe`. No third-party dependencies. Like the underlying codec,
//! this module is an independent re-implementation of SPEC; the Ruby
//! host envelope module ends up byte-compatible because both sides
//! follow SPEC, not because one was copied from the other.

use crate::codec::{CodecError, Decoder, Encoder, Value};

/// Response variant marker for the success branch
/// (docs/wire-codec.md § Envelope Encoding → Response). Module-private — `Response::Ok`
/// / `Response::Err` are the public surface and reify these values.
const STATUS_OK: i64 = 0;
/// Response variant marker for the error branch. Module-private.
const STATUS_ERROR: i64 = 1;

/// Errors raised by envelope-level encode/decode on top of [`CodecError`].
///
/// A pure codec fault (truncated input, bad UTF-8, etc.) bubbles up as
/// [`EnvelopeError::Codec`]. Envelope-shape faults (wrong arity, missing
/// required field, illegal tag byte) get their own variants so the host
/// can classify them per SPEC's attribution rules.
///
/// Shared with `crate::outcome` — both layers raise codec-shape faults
/// the same way, and deduplicating the error type avoids a parallel
/// hierarchy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnvelopeError {
    /// Underlying codec rejected the input bytes.
    Codec(CodecError),
    /// The decoded value does not match the SPEC envelope shape (e.g.
    /// Request was not a 5-element array, Response status was outside
    /// {0, 1}, Outcome tag byte was neither 0x01 nor 0x02).
    Shape(&'static str),
    /// A required field was missing from a Panic envelope (SPEC pins
    /// "origin", "class", "message" as mandatory).
    MissingField(&'static str),
    /// A field had the wrong msgpack family (e.g. Request `target` was
    /// neither str nor Handle).
    WrongFieldType(&'static str),
}

impl From<CodecError> for EnvelopeError {
    fn from(e: CodecError) -> Self {
        EnvelopeError::Codec(e)
    }
}

impl std::fmt::Display for EnvelopeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EnvelopeError::Codec(e) => write!(f, "codec rejected envelope bytes: {e}"),
            EnvelopeError::Shape(msg) => write!(f, "envelope shape mismatch: {msg}"),
            EnvelopeError::MissingField(name) => {
                write!(f, "envelope missing required field: {name}")
            }
            EnvelopeError::WrongFieldType(msg) => write!(f, "envelope field had wrong type: {msg}"),
        }
    }
}

impl std::error::Error for EnvelopeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            EnvelopeError::Codec(e) => Some(e),
            _ => None,
        }
    }
}

// ============================================================
// Value objects
// ============================================================

/// docs/wire-codec.md § Envelope Encoding → Request: 5-element msgpack
/// array `[target, method, args, kwargs, block_given]`. `target` is
/// either a Member constant path (str, e.g. `"Namespace::Member"`) or a
/// Capability Handle. `block_given` is a Boolean signalling whether the
/// guest call site supplied a block (B-23); the block body itself never
/// crosses the wire.
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    pub target: Target,
    pub method: String,
    pub args: Vec<Value>,
    pub kwargs: Vec<(String, Value)>,
    pub block_given: bool,
}

/// The two distinguishable forms of a Request `target` (docs/wire-codec.md
/// § Envelope Encoding → Request: "the two forms are distinguishable on
/// the wire").
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// Member constant path, e.g. `"Namespace::Member"`.
    Path(String),
    /// Capability Handle reference (ext 0x01).
    Handle(u32),
}

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

// ============================================================
// Encode / decode
// ============================================================

// ---------------- Request ----------------

/// Encode a [`Request`] to its 5-field msgpack array bytes. Per SPEC
/// (docs/wire-codec.md § Ext Types → ext 0x00) `kwargs` keys are
/// emitted as Symbols, so we emit [`Value::Sym`] at every kwargs-key
/// slot.
pub fn encode_request(req: &Request) -> Result<Vec<u8>, EnvelopeError> {
    let target_value = match &req.target {
        Target::Path(s) => Value::Str(s.clone()),
        Target::Handle(id) => Value::Handle(*id),
    };
    let kwargs_pairs: Vec<(Value, Value)> = req
        .kwargs
        .iter()
        .map(|(k, v)| (Value::Sym(k.clone()), v.clone()))
        .collect();
    let frame = Value::Array(vec![
        target_value,
        Value::Str(req.method.clone()),
        Value::Array(req.args.clone()),
        Value::Map(kwargs_pairs),
        Value::Bool(req.block_given),
    ]);
    let mut enc = Encoder::new();
    enc.write_value(&frame)?;
    Ok(enc.into_bytes())
}

/// Decode bytes to a [`Request`].
pub fn decode_request(bytes: &[u8]) -> Result<Request, EnvelopeError> {
    let mut dec = Decoder::new(bytes);
    let frame = dec.read_value()?;
    // `try_into` on a Vec succeeds iff length matches; the preceding guard
    // makes that condition true, so the unwrap is unreachable in practice.
    let [target_v, method_v, args_v, kwargs_v, block_given_v]: [Value; 5] = match frame {
        Value::Array(items) if items.len() == 5 => items.try_into().unwrap(),
        _ => return Err(EnvelopeError::Shape("Request must be a 5-element array")),
    };

    let target = match target_v {
        Value::Str(s) => Target::Path(s),
        Value::Handle(id) => Target::Handle(id),
        _ => {
            return Err(EnvelopeError::WrongFieldType(
                "Request target must be str or Handle",
            ))
        }
    };
    let method = match method_v {
        Value::Str(s) => s,
        _ => return Err(EnvelopeError::WrongFieldType("Request method must be str")),
    };
    let args = match args_v {
        Value::Array(items) => items,
        _ => return Err(EnvelopeError::WrongFieldType("Request args must be array")),
    };
    let kwargs = match kwargs_v {
        Value::Map(pairs) => {
            let mut out = Vec::with_capacity(pairs.len());
            for (k, v) in pairs {
                let key = match k {
                    Value::Sym(s) => s,
                    _ => {
                        return Err(EnvelopeError::WrongFieldType(
                            "Request kwargs keys must be Symbol (ext 0x00)",
                        ))
                    }
                };
                out.push((key, v));
            }
            out
        }
        _ => return Err(EnvelopeError::WrongFieldType("Request kwargs must be map")),
    };
    let block_given = match block_given_v {
        Value::Bool(b) => b,
        _ => {
            return Err(EnvelopeError::WrongFieldType(
                "Request block_given must be bool",
            ))
        }
    };
    Ok(Request {
        target,
        method,
        args,
        kwargs,
        block_given,
    })
}

// ---------------- Response ----------------

pub fn encode_response(resp: &Response) -> Result<Vec<u8>, EnvelopeError> {
    let (status, payload) = match resp {
        Response::Ok(v) => (STATUS_OK, v.clone()),
        Response::Err(payload_bytes) => (STATUS_ERROR, Value::ErrEnv(payload_bytes.clone())),
    };
    let frame = Value::Array(vec![Value::Int(status), payload]);
    let mut enc = Encoder::new();
    enc.write_value(&frame)?;
    Ok(enc.into_bytes())
}

pub fn decode_response(bytes: &[u8]) -> Result<Response, EnvelopeError> {
    let mut dec = Decoder::new(bytes);
    let frame = dec.read_value()?;
    let [status, payload]: [Value; 2] = match frame {
        Value::Array(items) if items.len() == 2 => items.try_into().unwrap(),
        _ => return Err(EnvelopeError::Shape("Response must be a 2-element array")),
    };
    let status = match status {
        Value::Int(n) => n,
        _ => return Err(EnvelopeError::WrongFieldType("Response status must be int")),
    };
    match status {
        STATUS_OK => Ok(Response::Ok(payload)),
        STATUS_ERROR => match payload {
            Value::ErrEnv(bytes) => Ok(Response::Err(bytes)),
            _ => Err(EnvelopeError::WrongFieldType(
                "Response status=1 payload must be ext 0x02 Fault",
            )),
        },
        _ => Err(EnvelopeError::Shape("Response status must be 0 or 1")),
    }
}

// ============================================================
// Tests
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

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

    // ---------------- Request ----------------

    #[test]
    fn request_round_trip_with_path_target() {
        let req = Request {
            target: Target::Path("Store::Users".into()),
            method: "find".into(),
            args: vec![Value::Int(42), Value::Str("alice".into())],
            kwargs: vec![("active".into(), Value::Bool(true))],
            block_given: false,
        };
        let bytes = encode_request(&req).unwrap();
        let out = decode_request(&bytes).unwrap();
        assert_eq!(req, out);
    }

    #[test]
    fn request_round_trip_with_handle_target() {
        let req = Request {
            target: Target::Handle(7),
            method: "save".into(),
            args: vec![],
            kwargs: vec![],
            block_given: false,
        };
        let bytes = encode_request(&req).unwrap();
        let out = decode_request(&bytes).unwrap();
        assert_eq!(req, out);
    }

    #[test]
    fn request_with_handle_in_args() {
        let req = Request {
            target: Target::Path("G::M".into()),
            method: "link".into(),
            args: vec![Value::Handle(1), Value::Handle(2), Value::Str("tag".into())],
            kwargs: vec![("k".into(), Value::Handle(1))],
            block_given: false,
        };
        let bytes = encode_request(&req).unwrap();
        assert_eq!(decode_request(&bytes).unwrap(), req);
    }

    #[test]
    fn request_round_trip_with_block_given_true() {
        let req = Request {
            target: Target::Path("Each::Iter".into()),
            method: "run".into(),
            args: vec![Value::Array(vec![Value::Int(1), Value::Int(2)])],
            kwargs: vec![],
            block_given: true,
        };
        let bytes = encode_request(&req).unwrap();
        let out = decode_request(&bytes).unwrap();
        assert_eq!(req, out);
        assert!(out.block_given);
    }

    #[test]
    fn request_decode_rejects_wrong_arity() {
        let mut enc = Encoder::new();
        // 4-element array — post-B-23 the Request envelope carries
        // `block_given` as the 5th element.
        enc.write_value(&Value::Array(vec![
            Value::Str("G::M".into()),
            Value::Str("x".into()),
            Value::Array(vec![]),
            Value::Map(vec![]),
        ]))
        .unwrap();
        assert!(matches!(
            decode_request(&enc.into_bytes()),
            Err(EnvelopeError::Shape(_))
        ));
    }

    #[test]
    fn request_decode_rejects_non_bool_block_given() {
        let mut enc = Encoder::new();
        enc.write_value(&Value::Array(vec![
            Value::Str("G::M".into()),
            Value::Str("x".into()),
            Value::Array(vec![]),
            Value::Map(vec![]),
            Value::Int(0),
        ]))
        .unwrap();
        assert!(matches!(
            decode_request(&enc.into_bytes()),
            Err(EnvelopeError::WrongFieldType(_))
        ));
    }

    #[test]
    fn request_golden_empty_args_and_kwargs() {
        let req = Request {
            target: Target::Path("G::M".into()),
            method: "ping".into(),
            args: vec![],
            kwargs: vec![],
            block_given: false,
        };
        let bytes = encode_request(&req).unwrap();
        // Same hex as the Ruby golden test in test_rpc_envelope.rb.
        assert_eq!(
            bytes,
            vec![
                0x95, // fixarray 5
                0xa4, b'G', b':', b':', b'M', // fixstr 4 "G::M"
                0xa4, b'p', b'i', b'n', b'g', // fixstr 4 "ping"
                0x90, // fixarray 0
                0x80, // fixmap 0
                0xc2, // false
            ]
        );
    }

    // ---------------- Response ----------------

    #[test]
    fn response_ok_round_trip_with_primitive() {
        let resp = Response::Ok(Value::Int(42));
        let out = decode_response(&encode_response(&resp).unwrap()).unwrap();
        assert_eq!(resp, out);
    }

    #[test]
    fn response_ok_round_trip_with_handle() {
        let resp = Response::Ok(Value::Handle(99));
        let out = decode_response(&encode_response(&resp).unwrap()).unwrap();
        assert_eq!(resp, out);
    }

    #[test]
    fn response_err_round_trip() {
        let payload = errenv_payload("runtime", "boom");
        let resp = Response::Err(payload);
        let out = decode_response(&encode_response(&resp).unwrap()).unwrap();
        assert_eq!(resp, out);
    }

    #[test]
    fn response_decode_rejects_status_two() {
        let mut enc = Encoder::new();
        enc.write_value(&Value::Array(vec![Value::Int(2), Value::Nil]))
            .unwrap();
        assert!(matches!(
            decode_response(&enc.into_bytes()),
            Err(EnvelopeError::Shape(_))
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
            decode_response(&enc.into_bytes()),
            Err(EnvelopeError::WrongFieldType(_))
        ));
    }

    #[test]
    fn response_ok_golden_for_42() {
        let bytes = encode_response(&Response::Ok(Value::Int(42))).unwrap();
        assert_eq!(bytes, vec![0x92, 0x00, 0x2a]);
    }
}
