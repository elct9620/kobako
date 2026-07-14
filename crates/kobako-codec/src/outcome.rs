//! Per-run Outcome envelope encoders/decoders.
//!
//! docs/wire-contract.md § Outcome Envelope wraps a single `#run` invocation's final
//! result (`Result` branch — the user script's last expression) or
//! top-level uncaught exception (`Panic` branch). This module mirrors the
//! host's `lib/kobako/outcome.rb` + `lib/kobako/outcome/panic.rb`: the
//! per-run `Outcome` framing lives here, the `Panic` wire record in the
//! `panic` submodule, distinct from the per-transport-call envelopes in
//! `transport/{request,response}.rs`.
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

pub mod panic;
pub use panic::Panic;

/// Outcome envelope tag for a Result envelope (docs/wire-contract.md § Outcome
/// Envelope). `Outcome::Value` reifies this value; the constant is
/// exposed for frontends that split the tag themselves to own their
/// per-stage failure attribution.
pub const OUTCOME_TAG_RESULT: u8 = 0x01;

/// Outcome envelope tag for a Panic envelope (docs/wire-contract.md § Outcome
/// Envelope). Exposed alongside `OUTCOME_TAG_RESULT`.
pub const OUTCOME_TAG_PANIC: u8 = 0x02;

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

impl Encode for Outcome {
    /// Encode an Outcome: a 1-byte tag (`0x01` value / `0x02` panic)
    /// followed by the branch payload. The value branch is the bare
    /// msgpack encoding of the carried value (no enclosing wrapper, per
    /// docs/wire-contract.md § Outcome Envelope); the panic branch delegates
    /// to `Panic`'s own codec.
    fn encode(&self) -> Result<Vec<u8>, codec::Error> {
        let (tag, body) = match self {
            Outcome::Value(v) => (OUTCOME_TAG_RESULT, Encoder::encode(v)?),
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
                let value = dec.read_only_value()?;
                // A Result envelope is a payload position: the Fault
                // envelope's only home is the Response fault field, so an
                // ext 0x02 in the carried value is a wire violation.
                if value.contains_errenv() {
                    return Err(codec::Error::Malformed(
                        "Fault envelope (ext 0x02) is not a legal value in a Result envelope",
                    ));
                }
                Ok(Outcome::Value(value))
            }
            OUTCOME_TAG_PANIC => Ok(Outcome::Panic(Panic::decode(body)?)),
            _ => Err(codec::Error::Malformed("Outcome tag must be 0x01 or 0x02")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // E-50: a Result envelope smuggling an ext 0x02 — even nested — must
    // fail decode.
    #[test]
    fn outcome_value_decode_rejects_errenv() {
        let o = Outcome::Value(Value::Array(vec![Value::ErrEnv(vec![0x80])]));
        let bytes = o.encode().unwrap();
        assert!(matches!(
            Outcome::decode(&bytes),
            Err(codec::Error::Malformed(_))
        ));
    }

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
