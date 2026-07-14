//! Run envelope — the host→guest entrypoint-dispatch value object + its
//! wire codec.
//!
//! docs/wire-codec.md § Invocation channels → Invocation envelope pins
//! the binary framing consumed by `__kobako_run`. Mirrors the host's
//! `lib/kobako/transport/run.rb` on the wire; host pre-flight (entrypoint
//! pattern, forged-Handle rejection) stays at each frontend's surface so
//! this type remains a pure wire carrier both sides can share.
//!
//! No `unsafe`. No third-party dependencies. An independent
//! re-implementation of SPEC; byte-compatible with the host because both
//! follow SPEC, not because one was copied from the other.

use crate::codec::{self, Decoder, Encoder, Value};

/// docs/wire-codec.md § Invocation channels → Invocation envelope:
/// msgpack map of `"entrypoint"` (Symbol, ext 0x00), `"args"` (Array),
/// and `"kwargs"` (Map with Symbol keys). Empty `args` / `kwargs` ride
/// as explicit empty collections, never absent.
#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    pub entrypoint: String,
    pub args: Vec<Value>,
    pub kwargs: Vec<(String, Value)>,
}

impl codec::Encode for Run {
    /// Encode a `Run` to its 3-key msgpack map bytes. The entrypoint
    /// rides as a Symbol (the host normalizes String input before the
    /// envelope is built) and every kwargs key is emitted as a Symbol,
    /// per docs/wire-codec.md § Ext Types → ext 0x00.
    fn encode(&self) -> Result<Vec<u8>, codec::Error> {
        let kwargs_pairs: Vec<(Value, Value)> = self
            .kwargs
            .iter()
            .map(|(k, v)| (Value::Sym(k.clone()), v.clone()))
            .collect();
        let frame = Value::Map(vec![
            (
                Value::Str("entrypoint".into()),
                Value::Sym(self.entrypoint.clone()),
            ),
            (Value::Str("args".into()), Value::Array(self.args.clone())),
            (Value::Str("kwargs".into()), Value::Map(kwargs_pairs)),
        ]);
        let mut enc = Encoder::new();
        enc.write_value(&frame)?;
        Ok(enc.into_bytes())
    }
}

impl codec::Decode for Run {
    /// Decode bytes to a `Run`. Strict to the host encoder's contract:
    /// all three keys present, entrypoint a Symbol, kwargs keys Symbols.
    /// (The guest's own reader stays tolerant of absent collections;
    /// this decoder pins what a conforming host must have emitted.)
    fn decode(bytes: &[u8]) -> Result<Self, codec::Error> {
        let mut dec = Decoder::new(bytes);
        let frame = dec.read_only_value()?;
        let Value::Map(pairs) = frame else {
            return Err(codec::Error::Malformed("Run must be a map"));
        };

        let mut entrypoint = None;
        let mut args = None;
        let mut kwargs = None;
        for (key, value) in pairs {
            let Value::Str(key) = key else {
                return Err(codec::Error::Malformed("Run keys must be str"));
            };
            match key.as_str() {
                "entrypoint" => match value {
                    Value::Sym(name) => entrypoint = Some(name),
                    _ => {
                        return Err(codec::Error::Malformed(
                            "Run entrypoint must be Symbol (ext 0x00)",
                        ))
                    }
                },
                "args" => match value {
                    Value::Array(items) => args = Some(items),
                    _ => return Err(codec::Error::Malformed("Run args must be array")),
                },
                "kwargs" => match value {
                    Value::Map(entries) => {
                        let mut out = Vec::with_capacity(entries.len());
                        for (k, v) in entries {
                            let Value::Sym(name) = k else {
                                return Err(codec::Error::Malformed(
                                    "Run kwargs keys must be Symbol (ext 0x00)",
                                ));
                            };
                            out.push((name, v));
                        }
                        kwargs = Some(out);
                    }
                    _ => return Err(codec::Error::Malformed("Run kwargs must be map")),
                },
                _ => return Err(codec::Error::Malformed("Run carries an unknown key")),
            }
        }

        Ok(Run {
            entrypoint: entrypoint
                .ok_or(codec::Error::Malformed("Run must carry an entrypoint"))?,
            args: args.ok_or(codec::Error::Malformed("Run must carry args"))?,
            kwargs: kwargs.ok_or(codec::Error::Malformed("Run must carry kwargs"))?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{Decode, Encode};

    #[test]
    fn run_round_trip_with_args_and_kwargs() {
        let run = Run {
            entrypoint: "Handler".into(),
            args: vec![Value::Int(42), Value::Str("alice".into())],
            kwargs: vec![
                ("active".into(), Value::Bool(true)),
                ("tag".into(), Value::Sym("hot".into())),
            ],
        };
        let bytes = run.encode().unwrap();
        assert_eq!(Run::decode(&bytes).unwrap(), run);
    }

    #[test]
    fn run_golden_empty_args_and_kwargs() {
        let run = Run {
            entrypoint: "Main".into(),
            args: vec![],
            kwargs: vec![],
        };
        // The wire image the Ruby host emits for
        // `Transport::Run.new(entrypoint: :Main).encode(handles)`.
        assert_eq!(
            run.encode().unwrap(),
            vec![
                0x83, // fixmap 3
                0xaa, b'e', b'n', b't', b'r', b'y', b'p', b'o', b'i', b'n', b't', 0xd6, 0x00, b'M',
                b'a', b'i', b'n', // fixext4, ext 0x00 Symbol
                0xa4, b'a', b'r', b'g', b's', 0x90, // fixarray 0
                0xa6, b'k', b'w', b'a', b'r', b'g', b's', 0x80, // fixmap 0
            ]
        );
    }

    // The Run decoder pins the map shape a conforming host must emit; each
    // test below drives one refusal arm and matches its exact message, so
    // dropping or swapping an arm turns the matching test red.
    #[test]
    fn run_decode_rejects_non_map_frame() {
        let mut enc = Encoder::new();
        enc.write_value(&Value::Array(vec![])).unwrap();
        assert!(matches!(
            Run::decode(&enc.into_bytes()),
            Err(codec::Error::Malformed("Run must be a map"))
        ));
    }

    #[test]
    fn run_decode_rejects_non_str_key() {
        let mut enc = Encoder::new();
        enc.write_value(&Value::Map(vec![(Value::Int(1), Value::Nil)]))
            .unwrap();
        assert!(matches!(
            Run::decode(&enc.into_bytes()),
            Err(codec::Error::Malformed("Run keys must be str"))
        ));
    }

    #[test]
    fn run_decode_rejects_non_array_args() {
        let mut enc = Encoder::new();
        enc.write_value(&Value::Map(vec![
            (Value::Str("entrypoint".into()), Value::Sym("Main".into())),
            (Value::Str("args".into()), Value::Int(0)),
            (Value::Str("kwargs".into()), Value::Map(vec![])),
        ]))
        .unwrap();
        assert!(matches!(
            Run::decode(&enc.into_bytes()),
            Err(codec::Error::Malformed("Run args must be array"))
        ));
    }

    #[test]
    fn run_decode_rejects_non_map_kwargs() {
        let mut enc = Encoder::new();
        enc.write_value(&Value::Map(vec![
            (Value::Str("entrypoint".into()), Value::Sym("Main".into())),
            (Value::Str("args".into()), Value::Array(vec![])),
            (Value::Str("kwargs".into()), Value::Int(0)),
        ]))
        .unwrap();
        assert!(matches!(
            Run::decode(&enc.into_bytes()),
            Err(codec::Error::Malformed("Run kwargs must be map"))
        ));
    }

    #[test]
    fn run_decode_rejects_unknown_key() {
        let mut enc = Encoder::new();
        enc.write_value(&Value::Map(vec![(
            Value::Str("surprise".into()),
            Value::Nil,
        )]))
        .unwrap();
        assert!(matches!(
            Run::decode(&enc.into_bytes()),
            Err(codec::Error::Malformed("Run carries an unknown key"))
        ));
    }

    #[test]
    fn run_decode_rejects_non_symbol_entrypoint() {
        let mut enc = Encoder::new();
        enc.write_value(&Value::Map(vec![
            (
                Value::Str("entrypoint".into()),
                Value::Str("Main".into()), // str where the wire demands ext 0x00
            ),
            (Value::Str("args".into()), Value::Array(vec![])),
            (Value::Str("kwargs".into()), Value::Map(vec![])),
        ]))
        .unwrap();
        assert!(matches!(
            Run::decode(&enc.into_bytes()),
            Err(codec::Error::Malformed(_))
        ));
    }

    #[test]
    fn run_decode_rejects_missing_entrypoint() {
        let mut enc = Encoder::new();
        enc.write_value(&Value::Map(vec![
            (Value::Str("args".into()), Value::Array(vec![])),
            (Value::Str("kwargs".into()), Value::Map(vec![])),
        ]))
        .unwrap();
        assert!(matches!(
            Run::decode(&enc.into_bytes()),
            Err(codec::Error::Malformed(_))
        ));
    }

    #[test]
    fn run_decode_rejects_non_symbol_kwargs_key() {
        let mut enc = Encoder::new();
        enc.write_value(&Value::Map(vec![
            (Value::Str("entrypoint".into()), Value::Sym("Main".into())),
            (Value::Str("args".into()), Value::Array(vec![])),
            (
                Value::Str("kwargs".into()),
                Value::Map(vec![(Value::Str("limit".into()), Value::Int(1))]),
            ),
        ]))
        .unwrap();
        assert!(matches!(
            Run::decode(&enc.into_bytes()),
            Err(codec::Error::Malformed(_))
        ));
    }

    #[test]
    fn run_decode_rejects_trailing_bytes() {
        let run = Run {
            entrypoint: "Handler".into(),
            args: vec![],
            kwargs: vec![],
        };
        let mut bytes = run.encode().unwrap();
        bytes.push(0xc0); // a second msgpack value after the envelope
        assert!(matches!(
            Run::decode(&bytes),
            Err(codec::Error::Malformed(_))
        ));
    }
}
