//! Request envelope — the guest→host call value object + its wire codec.
//!
//! docs/wire-codec.md § Envelope Encoding → Request pins the binary
//! framing; this assembles it on top of the lower-level `Encoder` /
//! `Decoder` primitives in `codec/`. Mirrors the host's
//! `lib/kobako/transport/request.rb`.
//!
//! No `unsafe`. No third-party dependencies. An independent
//! re-implementation of SPEC; byte-compatible with the host because both
//! follow SPEC, not because one was copied from the other.

use crate::codec::{self, Decoder, Encoder, Value};

/// docs/wire-codec.md § Envelope Encoding → Request: 5-element msgpack
/// array `[target, method, args, kwargs, block_given]`. `target` is
/// either a Member constant path (str of the form `"MyService::KV"`,
/// e.g. `"MyService::KV"`) or a Capability Handle. `block_given` is a
/// Boolean signalling whether the
/// guest call site supplied a block; the block body itself never
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
    /// Member constant path of the form `"MyService::KV"`
    /// (e.g. `"MyService::KV"`).
    Path(String),
    /// Capability Handle reference (ext 0x01).
    Handle(u32),
}

impl codec::Encode for Request {
    /// Encode a `Request` to its 5-field msgpack array bytes. Per SPEC
    /// (docs/wire-codec.md § Ext Types → ext 0x00) `kwargs` keys are
    /// emitted as Symbols, so we emit `Value::Sym` at every kwargs-key
    /// slot.
    fn encode(&self) -> Result<Vec<u8>, codec::Error> {
        let target_value = match &self.target {
            Target::Path(s) => Value::Str(s.clone()),
            Target::Handle(id) => Value::Handle(*id),
        };
        let kwargs_pairs: Vec<(Value, Value)> = self
            .kwargs
            .iter()
            .map(|(k, v)| (Value::Sym(k.clone()), v.clone()))
            .collect();
        let frame = Value::Array(vec![
            target_value,
            Value::Str(self.method.clone()),
            Value::Array(self.args.clone()),
            Value::Map(kwargs_pairs),
            Value::Bool(self.block_given),
        ]);
        Encoder::encode(&frame)
    }
}

impl codec::Decode for Request {
    /// Decode bytes to a `Request`.
    fn decode(bytes: &[u8]) -> Result<Self, codec::Error> {
        let mut dec = Decoder::new(bytes);
        let frame = dec.read_only_value()?;
        // `try_into` on a Vec succeeds iff length matches; the preceding
        // guard makes that condition true, so the unwrap is unreachable.
        let [target_v, method_v, args_v, kwargs_v, block_given_v]: [Value; 5] = match frame {
            Value::Array(items) if items.len() == 5 => items.try_into().unwrap(),
            _ => return Err(codec::Error::Malformed("Request must be a 5-element array")),
        };

        let target = match target_v {
            Value::Str(s) => Target::Path(s),
            Value::Handle(id) => Target::Handle(id),
            _ => {
                return Err(codec::Error::Malformed(
                    "Request target must be str or Handle",
                ))
            }
        };
        let method = match method_v {
            Value::Str(s) => s,
            _ => return Err(codec::Error::Malformed("Request method must be str")),
        };
        let args = match args_v {
            Value::Array(items) => items,
            _ => return Err(codec::Error::Malformed("Request args must be array")),
        };
        let kwargs = match kwargs_v {
            Value::Map(pairs) => {
                let mut out = Vec::with_capacity(pairs.len());
                for (k, v) in pairs {
                    let key = match k {
                        Value::Sym(s) => s,
                        _ => {
                            return Err(codec::Error::Malformed(
                                "Request kwargs keys must be Symbol (ext 0x00)",
                            ))
                        }
                    };
                    out.push((key, v));
                }
                out
            }
            _ => return Err(codec::Error::Malformed("Request kwargs must be map")),
        };
        let block_given = match block_given_v {
            Value::Bool(b) => b,
            _ => return Err(codec::Error::Malformed("Request block_given must be bool")),
        };
        // A Request is a payload position: the Fault envelope's only home
        // is the Response fault field, so an ext 0x02 anywhere in the
        // argument trees is a wire violation.
        if args.iter().any(Value::contains_errenv)
            || kwargs.iter().any(|(_, v)| v.contains_errenv())
        {
            return Err(codec::Error::Malformed(
                "Fault envelope (ext 0x02) is not a legal value in a Request",
            ));
        }
        Ok(Request {
            target,
            method,
            args,
            kwargs,
            block_given,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{Decode, Encode};

    // E-50: the Fault envelope's only home is the Response fault field; a
    // Request smuggling one in an argument tree must fail decode.
    #[test]
    fn decode_rejects_errenv_in_args() {
        let req = Request {
            target: Target::Path("Store::Users".into()),
            method: "find".into(),
            args: vec![Value::Map(vec![(
                Value::Str("e".into()),
                Value::ErrEnv(vec![0x80]),
            )])],
            kwargs: vec![],
            block_given: false,
        };
        let bytes = req.encode().unwrap();
        assert!(matches!(
            Request::decode(&bytes),
            Err(codec::Error::Malformed(_))
        ));
    }

    // E-10: a Request target that is neither a path string nor a Handle
    // (e.g. a raw integer) is an invalid wire payload in a dispatch
    // position; the guest-side decoder refuses it before any dispatch
    // reaches the host.
    #[test]
    fn decode_rejects_non_handle_target() {
        let frame = Value::Array(vec![
            Value::Int(7),
            Value::Str("find".into()),
            Value::Array(vec![]),
            Value::Map(vec![]),
            Value::Bool(false),
        ]);
        let mut enc = Encoder::new();
        enc.write_value(&frame).unwrap();
        assert!(matches!(
            Request::decode(&enc.into_bytes()),
            Err(codec::Error::Malformed(
                "Request target must be str or Handle"
            ))
        ));
    }

    #[test]
    fn request_round_trip_with_path_target() {
        let req = Request {
            target: Target::Path("Store::Users".into()),
            method: "find".into(),
            args: vec![Value::Int(42), Value::Str("alice".into())],
            kwargs: vec![("active".into(), Value::Bool(true))],
            block_given: false,
        };
        let bytes = req.encode().unwrap();
        let out = Request::decode(&bytes).unwrap();
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
        let bytes = req.encode().unwrap();
        let out = Request::decode(&bytes).unwrap();
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
        let bytes = req.encode().unwrap();
        assert_eq!(Request::decode(&bytes).unwrap(), req);
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
        let bytes = req.encode().unwrap();
        let out = Request::decode(&bytes).unwrap();
        assert_eq!(req, out);
        assert!(out.block_given);
    }

    #[test]
    fn request_decode_rejects_wrong_arity() {
        let mut enc = Encoder::new();
        // 4-element array — the Request envelope carries
        // `block_given` as the 5th element.
        enc.write_value(&Value::Array(vec![
            Value::Str("G::M".into()),
            Value::Str("x".into()),
            Value::Array(vec![]),
            Value::Map(vec![]),
        ]))
        .unwrap();
        assert!(matches!(
            Request::decode(&enc.into_bytes()),
            Err(codec::Error::Malformed(_))
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
            Request::decode(&enc.into_bytes()),
            Err(codec::Error::Malformed(_))
        ));
    }

    #[test]
    fn request_decode_rejects_trailing_bytes() {
        let req = Request {
            target: Target::Path("G::M".into()),
            method: "x".into(),
            args: vec![],
            kwargs: vec![],
            block_given: false,
        };
        let mut bytes = req.encode().unwrap();
        bytes.push(0xc0); // a second msgpack value after the envelope
        assert!(matches!(
            Request::decode(&bytes),
            Err(codec::Error::Malformed(_))
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
        let bytes = req.encode().unwrap();
        // Same hex as the Ruby golden test in test/transport/test_envelope.rb.
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
}
