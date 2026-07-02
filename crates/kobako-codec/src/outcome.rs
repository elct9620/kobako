//! Per-run Outcome envelope encoders/decoders.
//!
//! docs/wire-contract.md § Outcome Envelope wraps a single `#run` invocation's final
//! result (`Result` branch — the user script's last expression) or
//! top-level uncaught exception (`Panic` branch). This module mirrors the
//! host's `lib/kobako/outcome.rb` + `lib/kobako/outcome/panic.rb`: per-run
//! shape lives here at the crate top level, distinct from the
//! per-transport-call envelopes in `transport/{request,response}.rs`.
//!
//! Decode faults surface as `crate::codec::Error` — the same type the
//! byte-level codec raises — so a malformed Outcome shares one error
//! channel with a malformed value. This matches the host, which raises a
//! single `Codec::Error` for both.
//!
//! No `unsafe`. No third-party dependencies. Like the underlying codec,
//! this module is an independent re-implementation of SPEC; the Ruby
//! host outcome module ends up byte-compatible because both sides follow
//! SPEC, not because one was copied from the other.

use crate::codec::{self, Decode, Decoder, Encode, Encoder, Value};

/// Outcome envelope tag for a Result envelope (docs/wire-contract.md § Outcome
/// Envelope). Module-private — `Outcome::Value` is the public surface
/// and reifies this value.
const OUTCOME_TAG_RESULT: u8 = 0x01;

/// Outcome envelope tag for a Panic envelope (docs/wire-contract.md § Outcome
/// Envelope). Module-private.
const OUTCOME_TAG_PANIC: u8 = 0x02;

// ============================================================
// Value objects
// ============================================================

/// docs/wire-contract.md § Outcome Envelope → Panic envelope: msgpack **map** keyed by
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

/// docs/wire-contract.md § Outcome Envelope: 1-byte tag (`0x01` success-value,
/// `0x02` Panic) followed by the msgpack payload of the corresponding
/// branch. The success branch is the bare msgpack encoding of the
/// returned `Value`; the tag alone discriminates the variant, so no
/// enclosing wrapper is added.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Value(Value),
    Panic(Panic),
}

// ============================================================
// Encode / decode
// ============================================================

impl Encode for Panic {
    /// Encode the Panic body as a msgpack map keyed by name. The optional
    /// keys (`backtrace`, `details`) are omitted when empty / absent.
    fn encode(&self) -> Result<Vec<u8>, codec::Error> {
        let mut pairs: Vec<(Value, Value)> = Vec::with_capacity(5);
        pairs.push((Value::Str("origin".into()), Value::Str(self.origin.clone())));
        pairs.push((Value::Str("class".into()), Value::Str(self.class.clone())));
        pairs.push((
            Value::Str("message".into()),
            Value::Str(self.message.clone()),
        ));
        if !self.backtrace.is_empty() {
            let bt = self
                .backtrace
                .iter()
                .map(|s| Value::Str(s.clone()))
                .collect();
            pairs.push((Value::Str("backtrace".into()), Value::Array(bt)));
        }
        if let Some(d) = &self.details {
            pairs.push((Value::Str("details".into()), d.clone()));
        }
        let mut enc = Encoder::new();
        enc.write_value(&Value::Map(pairs))?;
        Ok(enc.into_bytes())
    }
}

impl Decode for Panic {
    fn decode(bytes: &[u8]) -> Result<Self, codec::Error> {
        let mut dec = Decoder::new(bytes);
        let frame = dec.read_only_value()?;
        let pairs = match frame {
            Value::Map(p) => p,
            _ => return Err(codec::Error::Malformed("Panic must be a map")),
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
                    _ => return Err(codec::Error::Malformed("Panic origin must be str")),
                },
                "class" => match v {
                    Value::Str(s) => class = Some(s),
                    _ => return Err(codec::Error::Malformed("Panic class must be str")),
                },
                "message" => match v {
                    Value::Str(s) => message = Some(s),
                    _ => return Err(codec::Error::Malformed("Panic message must be str")),
                },
                "backtrace" => match v {
                    Value::Array(items) => {
                        for line in items {
                            match line {
                                Value::Str(s) => backtrace.push(s),
                                _ => {
                                    return Err(codec::Error::Malformed(
                                        "Panic backtrace lines must be str",
                                    ))
                                }
                            }
                        }
                    }
                    _ => return Err(codec::Error::Malformed("Panic backtrace must be array")),
                },
                "details" => details = Some(v),
                _ => { /* SPEC: silently ignore unknown keys for forward-compat */ }
            }
        }
        Ok(Panic {
            origin: origin.ok_or(codec::Error::Malformed(
                "Panic missing required field: origin",
            ))?,
            class: class.ok_or(codec::Error::Malformed(
                "Panic missing required field: class",
            ))?,
            message: message.ok_or(codec::Error::Malformed(
                "Panic missing required field: message",
            ))?,
            backtrace,
            details,
        })
    }
}

impl Encode for Outcome {
    /// Encode an Outcome: a 1-byte tag (`0x01` value / `0x02` panic)
    /// followed by the branch payload. The value branch is the bare
    /// msgpack encoding of the carried value (no enclosing wrapper, per
    /// docs/wire-contract.md § Outcome Envelope); the panic branch delegates
    /// to `Panic`'s own codec.
    fn encode(&self) -> Result<Vec<u8>, codec::Error> {
        let (tag, body) = match self {
            Outcome::Value(v) => {
                let mut enc = Encoder::new();
                enc.write_value(v)?;
                (OUTCOME_TAG_RESULT, enc.into_bytes())
            }
            Outcome::Panic(p) => (OUTCOME_TAG_PANIC, p.encode()?),
        };
        let mut out = Vec::with_capacity(1 + body.len());
        out.push(tag);
        out.extend_from_slice(&body);
        Ok(out)
    }
}

impl Decode for Outcome {
    fn decode(bytes: &[u8]) -> Result<Self, codec::Error> {
        let Some((&tag, body)) = bytes.split_first() else {
            return Err(codec::Error::Malformed("Outcome bytes must not be empty"));
        };
        match tag {
            OUTCOME_TAG_RESULT => {
                let mut dec = Decoder::new(body);
                Ok(Outcome::Value(dec.read_only_value()?))
            }
            OUTCOME_TAG_PANIC => Ok(Outcome::Panic(Panic::decode(body)?)),
            _ => Err(codec::Error::Malformed("Outcome tag must be 0x01 or 0x02")),
        }
    }
}

// ============================================================
// Tests
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

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
        let out = Panic::decode(&p.encode().unwrap()).unwrap();
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
        let out = Panic::decode(&p.encode().unwrap()).unwrap();
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
        let p = Panic::decode(&enc.into_bytes()).unwrap();
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
            Panic::decode(&enc.into_bytes()),
            Err(codec::Error::Malformed(
                "Panic missing required field: class"
            ))
        ));
    }

    #[test]
    fn panic_decode_rejects_non_map_payload() {
        let mut enc = Encoder::new();
        enc.write_value(&Value::Array(vec![Value::Int(1)])).unwrap();
        assert!(matches!(
            Panic::decode(&enc.into_bytes()),
            Err(codec::Error::Malformed(_))
        ));
    }

    // ---------------- Outcome envelope ----------------

    #[test]
    fn outcome_result_round_trip() {
        let o = Outcome::Value(Value::Int(123));
        let bytes = o.encode().unwrap();
        assert_eq!(bytes[0], OUTCOME_TAG_RESULT);
        assert_eq!(Outcome::decode(&bytes).unwrap(), o);
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
        let bytes = o.encode().unwrap();
        assert_eq!(bytes[0], OUTCOME_TAG_PANIC);
        assert_eq!(Outcome::decode(&bytes).unwrap(), o);
    }

    #[test]
    fn outcome_decode_rejects_unknown_tag() {
        let bytes = [0x03_u8, 0x90];
        assert!(matches!(
            Outcome::decode(&bytes),
            Err(codec::Error::Malformed(_))
        ));
    }

    #[test]
    fn outcome_decode_rejects_empty_bytes() {
        assert!(matches!(
            Outcome::decode(&[]),
            Err(codec::Error::Malformed(_))
        ));
    }

    #[test]
    fn outcome_result_golden_for_42() {
        let bytes = Outcome::Value(Value::Int(42)).encode().unwrap();
        // Tag 0x01 + bare msgpack value 0x2a (no enclosing array).
        assert_eq!(bytes, vec![0x01, 0x2a]);
    }
}
