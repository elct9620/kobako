//! The MessagePack payload codec — what rides inside a core envelope's
//! opaque `payload` field.
//!
//! The envelope routes a message; this decides what the resolved method
//! receives. Splitting them is what lets an endpoint with its own schema
//! replace this module and keep the envelope, so nothing here may reach
//! for a routing field.
//!
//! [payload codec]: ../../../../docs/wire/payload-msgpack.md

use super::codec::{self, Decode, Decoder, Encode, Encoder, Value};

/// The invocation arguments a Call or a Run carries: a 2-element msgpack
/// array, `args` then `kwargs`. Both elements are always present, so field
/// positions stay stable when either is empty.
///
/// The positional-versus-keyword split lives here rather than in the
/// envelope because it is Ruby's call semantics, not the wire's: an
/// codec for a language without them carries whatever shape it needs and
/// the envelope is unchanged.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Arguments {
    pub args: Vec<Value>,
    pub kwargs: Vec<(String, Value)>,
}

impl Arguments {
    pub fn new(args: Vec<Value>, kwargs: Vec<(String, Value)>) -> Self {
        Arguments { args, kwargs }
    }
}

impl Encode for Arguments {
    /// SPEC pins `kwargs` keys to Symbols (ext 0x00), so every key slot is
    /// emitted as `Value::Sym`.
    fn encode(&self) -> Result<Vec<u8>, codec::Error> {
        let kwargs = self
            .kwargs
            .iter()
            .map(|(name, value)| (Value::Sym(name.clone()), value.clone()))
            .collect();
        Encoder::encode(&Value::Array(vec![
            Value::Array(self.args.clone()),
            Value::Map(kwargs),
        ]))
    }
}

impl Decode for Arguments {
    fn decode(bytes: &[u8]) -> Result<Self, codec::Error> {
        let mut decoder = Decoder::new(bytes);
        let frame = decoder.read_only_value()?;
        // `try_into` on a Vec succeeds iff the length matches, which the
        // guard establishes.
        let [args_value, kwargs_value]: [Value; 2] = match frame {
            Value::Array(items) if items.len() == 2 => items.try_into().unwrap(),
            _ => {
                return Err(codec::Error::Malformed(
                    "an invocation payload must be a 2-element array of args and kwargs",
                ))
            }
        };

        let args = match args_value {
            Value::Array(items) => items,
            _ => return Err(codec::Error::Malformed("payload args must be an array")),
        };
        let kwargs = match kwargs_value {
            Value::Map(pairs) => {
                let mut out = Vec::with_capacity(pairs.len());
                for (key, value) in pairs {
                    match key {
                        Value::Sym(name) => out.push((name, value)),
                        _ => {
                            return Err(codec::Error::Malformed(
                                "payload kwargs keys must be Symbol (ext 0x00)",
                            ))
                        }
                    }
                }
                out
            }
            _ => return Err(codec::Error::Malformed("payload kwargs must be a map")),
        };

        // A Fault's only legal position is a Reply's fault arm, which the
        // envelope discriminates; one inside an argument tree is a wire
        // violation this codec refuses (E-50).
        if args.iter().any(Value::contains_fault)
            || kwargs.iter().any(|(_, value)| value.contains_fault())
        {
            return Err(codec::Error::Malformed(
                "a Fault (ext 0x02) is not a legal value in an invocation payload",
            ));
        }
        Ok(Arguments { args, kwargs })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_positional_and_keyword_arguments() {
        let arguments = Arguments::new(
            vec![Value::Int(1), Value::Str("two".into())],
            vec![("force".into(), Value::Bool(true))],
        );
        let encoded = arguments.encode().unwrap();
        assert_eq!(
            Arguments::decode(&encoded),
            Ok(arguments),
            "an invocation payload must survive an encode and decode with both argument kinds"
        );
    }

    #[test]
    fn an_empty_payload_keeps_both_positions() {
        let encoded = Arguments::default().encode().unwrap();
        assert_eq!(
            encoded,
            vec![0x92, 0x90, 0x80],
            "a call with no arguments must still emit both the args array and the kwargs map"
        );
        assert_eq!(
            Arguments::decode(&encoded),
            Ok(Arguments::default()),
            "an empty invocation payload must decode as empty rather than as a malformed frame"
        );
    }

    #[test]
    fn kwargs_keys_ride_as_symbols() {
        let encoded = Arguments::new(Vec::new(), vec![("name".into(), Value::Nil)])
            .encode()
            .unwrap();
        let frame = Decoder::new(&encoded).read_only_value().unwrap();
        let kwargs = match frame {
            Value::Array(items) => items[1].clone(),
            other => panic!("expected a 2-element frame, got {other:?}"),
        };
        assert_eq!(
            kwargs,
            Value::Map(vec![(Value::Sym("name".into()), Value::Nil)]),
            "a kwargs key must reach the wire as an ext 0x00 Symbol, never as a str"
        );
    }

    #[test]
    fn a_string_kwargs_key_is_refused() {
        let bytes = Encoder::encode(&Value::Array(vec![
            Value::Array(Vec::new()),
            Value::Map(vec![(Value::Str("name".into()), Value::Nil)]),
        ]))
        .unwrap();
        assert!(
            Arguments::decode(&bytes).is_err(),
            "a kwargs key that is not a Symbol must be rejected as a wire violation"
        );
    }

    #[test]
    fn a_frame_of_the_wrong_arity_is_refused() {
        let bytes = Encoder::encode(&Value::Array(vec![Value::Array(Vec::new())])).unwrap();
        assert!(
            Arguments::decode(&bytes).is_err(),
            "an invocation payload that is not a 2-element array must be rejected"
        );
    }

    /// The embedded msgpack map an ext 0x02 frame carries.
    fn fault_body() -> Vec<u8> {
        Encoder::encode(&Value::Map(vec![
            (Value::Str("type".into()), Value::Str("runtime".into())),
            (Value::Str("message".into()), Value::Str("boom".into())),
        ]))
        .unwrap()
    }

    #[test]
    fn a_fault_inside_an_argument_is_refused() {
        let bytes = Encoder::encode(&Value::Array(vec![
            Value::Array(vec![Value::Fault(fault_body())]),
            Value::Map(Vec::new()),
        ]))
        .unwrap();
        assert!(
            Arguments::decode(&bytes).is_err(),
            "a Fault smuggled into an argument must be rejected — its only home is a Reply's fault arm"
        );
    }

    #[test]
    fn a_fault_nested_in_a_kwargs_value_is_refused() {
        let bytes = Encoder::encode(&Value::Array(vec![
            Value::Array(Vec::new()),
            Value::Map(vec![(
                Value::Sym("cause".into()),
                Value::Array(vec![Value::Fault(fault_body())]),
            )]),
        ]))
        .unwrap();
        assert!(
            Arguments::decode(&bytes).is_err(),
            "a Fault nested inside a kwargs value must be rejected as deeply as a bare one"
        );
    }
}
