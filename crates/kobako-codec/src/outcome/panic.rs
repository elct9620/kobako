//! The Panic branch of the Outcome envelope — a `#run` invocation's
//! top-level uncaught exception, mirror of the host's
//! `lib/kobako/outcome/panic.rb`.

use crate::codec::{self, Decode, Decoder, Encode, Encoder, Value};

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
        Encoder::encode(&Value::Map(pairs))
    }
}

impl Decode for Panic {
    fn decode(bytes: &[u8]) -> Result<Self, codec::Error> {
        let mut dec = Decoder::new(bytes);
        let frame = dec.read_only_value()?;
        // The Panic envelope is a payload position: the Fault envelope's
        // only home is the Response fault field, so an ext 0x02 anywhere
        // in the frame — ignored keys included — is a wire violation,
        // matching the Ruby peer's whole-frame forbid_faults bracket.
        if frame.contains_errenv() {
            return Err(codec::Error::Malformed(
                "Fault envelope (ext 0x02) is not a legal value in a Panic envelope",
            ));
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    // E-50: the Fault envelope's only home is the Response fault field; a
    // Panic envelope smuggling one in `details` must fail decode.
    #[test]
    fn panic_decode_rejects_errenv_in_details() {
        let p = Panic {
            origin: "sandbox".into(),
            class: "RuntimeError".into(),
            message: "boom".into(),
            backtrace: vec![],
            details: Some(Value::ErrEnv(vec![0x80])),
        };
        let bytes = p.encode().unwrap();
        assert!(matches!(
            Panic::decode(&bytes),
            Err(codec::Error::Malformed(_))
        ));
    }

    // E-50: the whole Panic frame is a payload position, so an ext 0x02
    // hiding under a key the decoder ignores must fail decode too — the
    // Ruby peer's whole-frame forbid_faults bracket rejects these bytes.
    #[test]
    fn panic_decode_rejects_errenv_under_ignored_key() {
        let frame = Value::Map(vec![
            (Value::Str("origin".into()), Value::Str("sandbox".into())),
            (
                Value::Str("class".into()),
                Value::Str("RuntimeError".into()),
            ),
            (Value::Str("message".into()), Value::Str("boom".into())),
            (Value::Str("extra".into()), Value::ErrEnv(vec![0x80])),
        ]);
        let bytes = Encoder::encode(&frame).unwrap();
        assert!(matches!(
            Panic::decode(&bytes),
            Err(codec::Error::Malformed(_))
        ));
    }

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

    #[test]
    fn panic_decode_ignores_non_str_key() {
        // SPEC: keys that are not str are silently skipped, same as unknown
        // str keys — a non-str key never aborts the decode.
        let mut enc = Encoder::new();
        enc.write_value(&Value::Map(vec![
            (Value::Str("origin".into()), Value::Str("sandbox".into())),
            (
                Value::Str("class".into()),
                Value::Str("RuntimeError".into()),
            ),
            (Value::Str("message".into()), Value::Str("boom".into())),
            (Value::Int(7), Value::Str("ignored".into())),
        ]))
        .unwrap();
        let p = Panic::decode(&enc.into_bytes()).unwrap();
        assert_eq!(p.origin, "sandbox");
    }

    // Each required/optional Panic field pins its codec type; a value of the
    // wrong type aborts the decode with that field's exact message, so a
    // dropped or swapped arm turns the matching test red.
    #[test]
    fn panic_decode_rejects_non_str_origin() {
        let mut enc = Encoder::new();
        enc.write_value(&Value::Map(vec![
            (Value::Str("origin".into()), Value::Int(0)),
            (
                Value::Str("class".into()),
                Value::Str("RuntimeError".into()),
            ),
            (Value::Str("message".into()), Value::Str("boom".into())),
        ]))
        .unwrap();
        assert!(matches!(
            Panic::decode(&enc.into_bytes()),
            Err(codec::Error::Malformed("Panic origin must be str"))
        ));
    }

    #[test]
    fn panic_decode_rejects_non_str_class() {
        let mut enc = Encoder::new();
        enc.write_value(&Value::Map(vec![
            (Value::Str("origin".into()), Value::Str("sandbox".into())),
            (Value::Str("class".into()), Value::Int(0)),
            (Value::Str("message".into()), Value::Str("boom".into())),
        ]))
        .unwrap();
        assert!(matches!(
            Panic::decode(&enc.into_bytes()),
            Err(codec::Error::Malformed("Panic class must be str"))
        ));
    }

    #[test]
    fn panic_decode_rejects_non_str_message() {
        let mut enc = Encoder::new();
        enc.write_value(&Value::Map(vec![
            (Value::Str("origin".into()), Value::Str("sandbox".into())),
            (
                Value::Str("class".into()),
                Value::Str("RuntimeError".into()),
            ),
            (Value::Str("message".into()), Value::Int(0)),
        ]))
        .unwrap();
        assert!(matches!(
            Panic::decode(&enc.into_bytes()),
            Err(codec::Error::Malformed("Panic message must be str"))
        ));
    }

    #[test]
    fn panic_decode_rejects_non_array_backtrace() {
        let mut enc = Encoder::new();
        enc.write_value(&Value::Map(vec![
            (Value::Str("origin".into()), Value::Str("sandbox".into())),
            (
                Value::Str("class".into()),
                Value::Str("RuntimeError".into()),
            ),
            (Value::Str("message".into()), Value::Str("boom".into())),
            (Value::Str("backtrace".into()), Value::Str("a.rb:1".into())),
        ]))
        .unwrap();
        assert!(matches!(
            Panic::decode(&enc.into_bytes()),
            Err(codec::Error::Malformed("Panic backtrace must be array"))
        ));
    }

    #[test]
    fn panic_decode_rejects_non_str_backtrace_line() {
        let mut enc = Encoder::new();
        enc.write_value(&Value::Map(vec![
            (Value::Str("origin".into()), Value::Str("sandbox".into())),
            (
                Value::Str("class".into()),
                Value::Str("RuntimeError".into()),
            ),
            (Value::Str("message".into()), Value::Str("boom".into())),
            (
                Value::Str("backtrace".into()),
                Value::Array(vec![Value::Str("a.rb:1".into()), Value::Int(2)]),
            ),
        ]))
        .unwrap();
        assert!(matches!(
            Panic::decode(&enc.into_bytes()),
            Err(codec::Error::Malformed("Panic backtrace lines must be str"))
        ));
    }
}
