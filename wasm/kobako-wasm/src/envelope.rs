//! Envelope-layer encoders/decoders for the kobako wire contract.
//!
//! SPEC.md → Wire Contract pins the logical shape of every host↔guest
//! message; SPEC.md → Wire Codec → Envelope Frame Layout pins the binary
//! framing. This module assembles the four envelope kinds (Request,
//! Response, Result, Panic) and the outer Outcome wrapper on top of the
//! lower-level [`Encoder`] / [`Decoder`] primitives in `codec/`.
//!
//! Renamed `Result` => [`ResultEnv`] in the public surface to avoid a
//! clash with `core::result::Result` in this crate's pervasive
//! `Result<T, EnvelopeError>` return type. The wire-layer concept is
//! still the SPEC's "Result envelope".
//!
//! No `unsafe`. No third-party dependencies. Like the underlying codec,
//! this module is an independent re-implementation of SPEC; the Ruby
//! host envelope module ends up byte-compatible because both sides
//! follow SPEC, not because one was copied from the other.

use crate::codec::{CodecError, Decoder, Encoder, Value};

/// Response variant marker for the success branch
/// (SPEC.md → Wire Codec → Response). Module-private — `Response::Ok`
/// / `Response::Err` are the public surface and reify these values.
const STATUS_OK: i64 = 0;
/// Response variant marker for the error branch. Module-private.
const STATUS_ERROR: i64 = 1;

/// Outcome envelope tag for a Result envelope (SPEC.md "Outcome
/// Envelope"). Module-private — `Outcome::Result` is the public
/// surface and reifies this value.
const OUTCOME_TAG_RESULT: u8 = 0x01;

/// Outcome envelope tag for a Panic envelope (SPEC.md "Outcome
/// Envelope"). Module-private.
const OUTCOME_TAG_PANIC: u8 = 0x02;

/// Errors raised by envelope-level encode/decode on top of [`CodecError`].
///
/// A pure codec fault (truncated input, bad UTF-8, etc.) bubbles up as
/// [`EnvelopeError::Codec`]. Envelope-shape faults (wrong arity, missing
/// required field, illegal tag byte) get their own variants so the host
/// can classify them per SPEC's attribution rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnvelopeError {
    /// Underlying wire codec rejected the input bytes.
    Codec(CodecError),
    /// The decoded value does not match the SPEC envelope shape (e.g.
    /// Request was not a 4-element array, Response status was outside
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
            EnvelopeError::Codec(e) => write!(f, "wire codec rejected envelope bytes: {e}"),
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

/// SPEC.md → Wire Codec → Request: 4-element msgpack array
/// `[target, method, args, kwargs]`. `target` is either a Service Member
/// constant path (str, e.g. `"Group::Member"`) or a Capability Handle.
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    pub target: Target,
    pub method: String,
    pub args: Vec<Value>,
    pub kwargs: Vec<(String, Value)>,
}

/// The two distinguishable forms of a Request `target` (SPEC.md → Wire
/// Codec → Request: "the two forms are distinguishable on the wire").
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// Service Member constant path, e.g. `"Group::Member"`.
    Path(String),
    /// Capability Handle reference (ext 0x01).
    Handle(u32),
}

/// SPEC.md → Wire Codec → Response: 2-element msgpack array
/// `[status, value-or-error-envelope]`. The two variants are
/// mutually exclusive.
#[derive(Debug, Clone, PartialEq)]
pub enum Response {
    /// Success: `status=0`, `value` carries the return value.
    Ok(Value),
    /// Error: `status=1`, payload is a SPEC ext 0x02 Exception envelope
    /// (we keep it as the raw payload bytes, matching `Value::ErrEnv`).
    Err(Vec<u8>),
}

/// SPEC.md → Outcome Envelope → Result envelope: 1-element msgpack
/// array carrying the deserialized last expression. Renamed in the
/// public surface to avoid a clash with `core::result::Result`.
#[derive(Debug, Clone, PartialEq)]
pub struct ResultEnv {
    pub value: Value,
}

/// SPEC.md → Outcome Envelope → Panic envelope: msgpack **map** keyed by
/// name (SPEC: unknown keys must be silently ignored). Required keys:
/// `"origin"`, `"class"`, `"message"`. Optional keys: `"backtrace"`,
/// `"details"`.
#[derive(Debug, Clone, PartialEq)]
pub struct Panic {
    pub origin: String,
    pub class: String,
    pub message: String,
    pub backtrace: Vec<String>,
    pub details: Option<Value>,
}

/// SPEC.md → Outcome Envelope: 1-byte tag (`0x01` Result, `0x02` Panic)
/// followed by the msgpack payload of the corresponding envelope.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Result(ResultEnv),
    Panic(Panic),
}

// ============================================================
// Encode / decode
// ============================================================

// ---------------- Request ----------------

/// Encode a [`Request`] to its 4-field msgpack array bytes.
pub fn encode_request(req: &Request) -> Result<Vec<u8>, EnvelopeError> {
    let target_value = match &req.target {
        Target::Path(s) => Value::Str(s.clone()),
        Target::Handle(id) => Value::Handle(*id),
    };
    let kwargs_pairs: Vec<(Value, Value)> = req
        .kwargs
        .iter()
        .map(|(k, v)| (Value::Str(k.clone()), v.clone()))
        .collect();
    let frame = Value::Array(vec![
        target_value,
        Value::Str(req.method.clone()),
        Value::Array(req.args.clone()),
        Value::Map(kwargs_pairs),
    ]);
    let mut enc = Encoder::new();
    enc.write_value(&frame)?;
    Ok(enc.into_bytes())
}

/// Decode bytes to a [`Request`].
pub fn decode_request(bytes: &[u8]) -> Result<Request, EnvelopeError> {
    let mut dec = Decoder::new(bytes);
    let frame = dec.read_value()?;
    let mut items = match frame {
        Value::Array(items) if items.len() == 4 => items,
        _ => return Err(EnvelopeError::Shape("Request must be a 4-element array")),
    };
    // Drain in order to avoid clones.
    let kwargs_v = items.pop().unwrap();
    let args_v = items.pop().unwrap();
    let method_v = items.pop().unwrap();
    let target_v = items.pop().unwrap();

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
                    Value::Str(s) => s,
                    _ => {
                        return Err(EnvelopeError::WrongFieldType(
                            "Request kwargs keys must be str",
                        ))
                    }
                };
                out.push((key, v));
            }
            out
        }
        _ => return Err(EnvelopeError::WrongFieldType("Request kwargs must be map")),
    };
    Ok(Request {
        target,
        method,
        args,
        kwargs,
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
    let mut items = match frame {
        Value::Array(items) if items.len() == 2 => items,
        _ => return Err(EnvelopeError::Shape("Response must be a 2-element array")),
    };
    let payload = items.pop().unwrap();
    let status = items.pop().unwrap();
    let status = match status {
        Value::Int(n) => n,
        _ => return Err(EnvelopeError::WrongFieldType("Response status must be int")),
    };
    match status {
        STATUS_OK => Ok(Response::Ok(payload)),
        STATUS_ERROR => match payload {
            Value::ErrEnv(bytes) => Ok(Response::Err(bytes)),
            _ => Err(EnvelopeError::WrongFieldType(
                "Response status=1 payload must be ext 0x02 Exception",
            )),
        },
        _ => Err(EnvelopeError::Shape("Response status must be 0 or 1")),
    }
}

// ---------------- Result envelope ----------------

pub fn encode_result(value: &Value) -> Result<Vec<u8>, EnvelopeError> {
    let mut enc = Encoder::new();
    enc.write_value(&Value::Array(vec![value.clone()]))?;
    Ok(enc.into_bytes())
}

pub fn decode_result(bytes: &[u8]) -> Result<ResultEnv, EnvelopeError> {
    let mut dec = Decoder::new(bytes);
    let frame = dec.read_value()?;
    let mut items = match frame {
        Value::Array(items) if items.len() == 1 => items,
        _ => {
            return Err(EnvelopeError::Shape(
                "Result envelope must be a 1-element array",
            ))
        }
    };
    Ok(ResultEnv {
        value: items.pop().unwrap(),
    })
}

// ---------------- Panic envelope ----------------

pub fn encode_panic(panic: &Panic) -> Result<Vec<u8>, EnvelopeError> {
    let mut pairs: Vec<(Value, Value)> = Vec::with_capacity(5);
    pairs.push((
        Value::Str("origin".into()),
        Value::Str(panic.origin.clone()),
    ));
    pairs.push((Value::Str("class".into()), Value::Str(panic.class.clone())));
    pairs.push((
        Value::Str("message".into()),
        Value::Str(panic.message.clone()),
    ));
    if !panic.backtrace.is_empty() {
        let bt = panic
            .backtrace
            .iter()
            .map(|s| Value::Str(s.clone()))
            .collect();
        pairs.push((Value::Str("backtrace".into()), Value::Array(bt)));
    }
    if let Some(d) = &panic.details {
        pairs.push((Value::Str("details".into()), d.clone()));
    }
    let mut enc = Encoder::new();
    enc.write_value(&Value::Map(pairs))?;
    Ok(enc.into_bytes())
}

pub fn decode_panic(bytes: &[u8]) -> Result<Panic, EnvelopeError> {
    let mut dec = Decoder::new(bytes);
    let frame = dec.read_value()?;
    let pairs = match frame {
        Value::Map(p) => p,
        _ => return Err(EnvelopeError::Shape("Panic envelope must be a map")),
    };
    let mut origin = None;
    let mut class = None;
    let mut message = None;
    let mut backtrace: Vec<String> = Vec::new();
    let mut details: Option<Value> = None;
    for (k, v) in pairs {
        let key = match k {
            Value::Str(s) => s,
            _ => continue, // SPEC: unknown / non-str keys are silently ignored
        };
        match key.as_str() {
            "origin" => match v {
                Value::Str(s) => origin = Some(s),
                _ => return Err(EnvelopeError::WrongFieldType("Panic origin must be str")),
            },
            "class" => match v {
                Value::Str(s) => class = Some(s),
                _ => return Err(EnvelopeError::WrongFieldType("Panic class must be str")),
            },
            "message" => match v {
                Value::Str(s) => message = Some(s),
                _ => return Err(EnvelopeError::WrongFieldType("Panic message must be str")),
            },
            "backtrace" => match v {
                Value::Array(items) => {
                    for line in items {
                        match line {
                            Value::Str(s) => backtrace.push(s),
                            _ => {
                                return Err(EnvelopeError::WrongFieldType(
                                    "Panic backtrace lines must be str",
                                ))
                            }
                        }
                    }
                }
                _ => {
                    return Err(EnvelopeError::WrongFieldType(
                        "Panic backtrace must be array",
                    ))
                }
            },
            "details" => details = Some(v),
            _ => { /* SPEC: silently ignore unknown keys for forward-compat */ }
        }
    }
    Ok(Panic {
        origin: origin.ok_or(EnvelopeError::MissingField("origin"))?,
        class: class.ok_or(EnvelopeError::MissingField("class"))?,
        message: message.ok_or(EnvelopeError::MissingField("message"))?,
        backtrace,
        details,
    })
}

// ---------------- Outcome envelope ----------------

pub fn encode_outcome(outcome: &Outcome) -> Result<Vec<u8>, EnvelopeError> {
    let (tag, body) = match outcome {
        Outcome::Result(r) => (OUTCOME_TAG_RESULT, encode_result(&r.value)?),
        Outcome::Panic(p) => (OUTCOME_TAG_PANIC, encode_panic(p)?),
    };
    let mut out = Vec::with_capacity(1 + body.len());
    out.push(tag);
    out.extend_from_slice(&body);
    Ok(out)
}

pub fn decode_outcome(bytes: &[u8]) -> Result<Outcome, EnvelopeError> {
    if bytes.is_empty() {
        return Err(EnvelopeError::Shape("Outcome bytes must not be empty"));
    }
    let tag = bytes[0];
    let body = &bytes[1..];
    match tag {
        OUTCOME_TAG_RESULT => Ok(Outcome::Result(decode_result(body)?)),
        OUTCOME_TAG_PANIC => Ok(Outcome::Panic(decode_panic(body)?)),
        _ => Err(EnvelopeError::Shape("Outcome tag must be 0x01 or 0x02")),
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
        };
        let bytes = encode_request(&req).unwrap();
        assert_eq!(decode_request(&bytes).unwrap(), req);
    }

    #[test]
    fn request_decode_rejects_wrong_arity() {
        let mut enc = Encoder::new();
        enc.write_value(&Value::Array(vec![
            Value::Str("G::M".into()),
            Value::Str("x".into()),
            Value::Array(vec![]),
        ]))
        .unwrap();
        assert!(matches!(
            decode_request(&enc.into_bytes()),
            Err(EnvelopeError::Shape(_))
        ));
    }

    #[test]
    fn request_golden_empty_args_and_kwargs() {
        let req = Request {
            target: Target::Path("G::M".into()),
            method: "ping".into(),
            args: vec![],
            kwargs: vec![],
        };
        let bytes = encode_request(&req).unwrap();
        // Same hex as the Ruby golden test in test_wire_envelope.rb.
        assert_eq!(
            bytes,
            vec![
                0x94, // fixarray 4
                0xa4, b'G', b':', b':', b'M', // fixstr 4 "G::M"
                0xa4, b'p', b'i', b'n', b'g', // fixstr 4 "ping"
                0x90, // fixarray 0
                0x80, // fixmap 0
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

    // ---------------- Result envelope ----------------

    #[test]
    fn result_round_trip_primitive() {
        let bytes = encode_result(&Value::Int(42)).unwrap();
        let out = decode_result(&bytes).unwrap();
        assert_eq!(out.value, Value::Int(42));
    }

    #[test]
    fn result_round_trip_nil() {
        let out = decode_result(&encode_result(&Value::Nil).unwrap()).unwrap();
        assert_eq!(out.value, Value::Nil);
    }

    #[test]
    fn result_round_trip_handle() {
        let out = decode_result(&encode_result(&Value::Handle(5)).unwrap()).unwrap();
        assert_eq!(out.value, Value::Handle(5));
    }

    #[test]
    fn result_golden_value_42() {
        let bytes = encode_result(&Value::Int(42)).unwrap();
        // Same hex as the Ruby golden test.
        assert_eq!(bytes, vec![0x91, 0x2a]);
    }

    // ---------------- Panic envelope ----------------

    #[test]
    fn panic_round_trip_minimum() {
        let p = Panic {
            origin: "sandbox".into(),
            class: "RuntimeError".into(),
            message: "boom".into(),
            backtrace: vec![],
            details: None,
        };
        let out = decode_panic(&encode_panic(&p).unwrap()).unwrap();
        assert_eq!(p, out);
    }

    #[test]
    fn panic_round_trip_with_backtrace_and_details() {
        let p = Panic {
            origin: "service".into(),
            class: "Kobako::ServiceError".into(),
            message: "service failed".into(),
            backtrace: vec!["a.rb:1".into(), "b.rb:2".into()],
            details: Some(Value::Map(vec![(
                Value::Str("type".into()),
                Value::Str("runtime".into()),
            )])),
        };
        let out = decode_panic(&encode_panic(&p).unwrap()).unwrap();
        assert_eq!(p, out);
    }

    #[test]
    fn panic_decode_silently_ignores_unknown_keys() {
        let mut enc = Encoder::new();
        enc.write_value(&Value::Map(vec![
            (Value::Str("origin".into()), Value::Str("sandbox".into())),
            (
                Value::Str("class".into()),
                Value::Str("RuntimeError".into()),
            ),
            (Value::Str("message".into()), Value::Str("boom".into())),
            (
                Value::Str("future_key".into()),
                Value::Str("ignored".into()),
            ),
        ]))
        .unwrap();
        let p = decode_panic(&enc.into_bytes()).unwrap();
        assert_eq!(p.origin, "sandbox");
    }

    #[test]
    fn panic_decode_rejects_missing_required_key() {
        let mut enc = Encoder::new();
        enc.write_value(&Value::Map(vec![
            (Value::Str("origin".into()), Value::Str("sandbox".into())),
            (Value::Str("message".into()), Value::Str("boom".into())),
        ]))
        .unwrap();
        assert!(matches!(
            decode_panic(&enc.into_bytes()),
            Err(EnvelopeError::MissingField("class"))
        ));
    }

    #[test]
    fn panic_decode_rejects_non_map_payload() {
        let mut enc = Encoder::new();
        enc.write_value(&Value::Array(vec![Value::Int(1)])).unwrap();
        assert!(matches!(
            decode_panic(&enc.into_bytes()),
            Err(EnvelopeError::Shape(_))
        ));
    }

    // ---------------- Outcome envelope ----------------

    #[test]
    fn outcome_result_round_trip() {
        let o = Outcome::Result(ResultEnv {
            value: Value::Int(123),
        });
        let bytes = encode_outcome(&o).unwrap();
        assert_eq!(bytes[0], OUTCOME_TAG_RESULT);
        assert_eq!(decode_outcome(&bytes).unwrap(), o);
    }

    #[test]
    fn outcome_panic_round_trip() {
        let p = Panic {
            origin: "sandbox".into(),
            class: "RuntimeError".into(),
            message: "boom".into(),
            backtrace: vec![],
            details: None,
        };
        let o = Outcome::Panic(p);
        let bytes = encode_outcome(&o).unwrap();
        assert_eq!(bytes[0], OUTCOME_TAG_PANIC);
        assert_eq!(decode_outcome(&bytes).unwrap(), o);
    }

    #[test]
    fn outcome_decode_rejects_unknown_tag() {
        let bytes = [0x03_u8, 0x90];
        assert!(matches!(
            decode_outcome(&bytes),
            Err(EnvelopeError::Shape(_))
        ));
    }

    #[test]
    fn outcome_decode_rejects_empty_bytes() {
        assert!(matches!(decode_outcome(&[]), Err(EnvelopeError::Shape(_))));
    }

    #[test]
    fn outcome_result_golden_for_42() {
        let bytes = encode_outcome(&Outcome::Result(ResultEnv {
            value: Value::Int(42),
        }))
        .unwrap();
        // Tag 0x01 + Result envelope (fixarray 1, 0x2a).
        assert_eq!(bytes, vec![0x01, 0x91, 0x2a]);
    }
}
