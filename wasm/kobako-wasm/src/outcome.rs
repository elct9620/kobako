//! Per-run Outcome envelope encoders/decoders.
//!
//! SPEC.md → Outcome Envelope wraps a single `#run` invocation's final
//! result (`Result` branch — the user script's last expression) or
//! top-level uncaught exception (`Panic` branch). This module mirrors the
//! host's `lib/kobako/outcome.rb` + `lib/kobako/outcome/panic.rb`: per-run
//! shape lives here at the crate top level, distinct from the per-RPC
//! envelope in `rpc/envelope.rs`.
//!
//! The error type [`EnvelopeError`] is shared with `rpc::envelope` — both
//! layers raise codec-level shape faults the same way; deduplicating it
//! avoids a parallel hierarchy.
//!
//! No `unsafe`. No third-party dependencies. Like the underlying codec,
//! this module is an independent re-implementation of SPEC; the Ruby
//! host outcome module ends up byte-compatible because both sides follow
//! SPEC, not because one was copied from the other.

use crate::codec::{Decoder, Encoder, Value};
use crate::rpc::envelope::EnvelopeError;

/// Outcome envelope tag for a Result envelope (SPEC.md "Outcome
/// Envelope"). Module-private — `Outcome::Value` is the public surface
/// and reifies this value.
const OUTCOME_TAG_RESULT: u8 = 0x01;

/// Outcome envelope tag for a Panic envelope (SPEC.md "Outcome
/// Envelope"). Module-private.
const OUTCOME_TAG_PANIC: u8 = 0x02;

// ============================================================
// Value objects
// ============================================================

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

/// SPEC.md → Outcome Envelope: 1-byte tag (`0x01` success-value,
/// `0x02` Panic) followed by the msgpack payload of the corresponding
/// branch. The success branch is the bare msgpack encoding of the
/// returned [`Value`]; the tag alone discriminates the variant, so no
/// enclosing wrapper is added.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Value(Value),
    Panic(Panic),
}

// ============================================================
// Encode / decode
// ============================================================

// ---------------- Result envelope ----------------

/// Encode the success branch of an Outcome: the raw msgpack encoding of
/// `value`. SPEC pins this as direct (no enclosing array); the Outcome
/// tag byte is the sole discriminator.
pub fn encode_result(value: &Value) -> Result<Vec<u8>, EnvelopeError> {
    let mut enc = Encoder::new();
    enc.write_value(value)?;
    Ok(enc.into_bytes())
}

/// Decode the success branch of an Outcome into the carried [`Value`].
pub fn decode_result(bytes: &[u8]) -> Result<Value, EnvelopeError> {
    let mut dec = Decoder::new(bytes);
    Ok(dec.read_value()?)
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
        Outcome::Value(v) => (OUTCOME_TAG_RESULT, encode_result(v)?),
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
        OUTCOME_TAG_RESULT => Ok(Outcome::Value(decode_result(body)?)),
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

    // ---------------- Result envelope ----------------

    #[test]
    fn result_round_trip_primitive() {
        let bytes = encode_result(&Value::Int(42)).unwrap();
        let out = decode_result(&bytes).unwrap();
        assert_eq!(out, Value::Int(42));
    }

    #[test]
    fn result_round_trip_nil() {
        let out = decode_result(&encode_result(&Value::Nil).unwrap()).unwrap();
        assert_eq!(out, Value::Nil);
    }

    #[test]
    fn result_round_trip_handle() {
        let out = decode_result(&encode_result(&Value::Handle(5)).unwrap()).unwrap();
        assert_eq!(out, Value::Handle(5));
    }

    #[test]
    fn result_golden_value_42() {
        let bytes = encode_result(&Value::Int(42)).unwrap();
        // SPEC.md → Result Envelope: the value is emitted directly without
        // an enclosing array, so the success body is just msgpack(42) = 0x2a.
        assert_eq!(bytes, vec![0x2a]);
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
        let o = Outcome::Value(Value::Int(123));
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
        let bytes = encode_outcome(&Outcome::Value(Value::Int(42))).unwrap();
        // Tag 0x01 + bare msgpack value 0x2a (no enclosing array).
        assert_eq!(bytes, vec![0x01, 0x2a]);
    }
}
