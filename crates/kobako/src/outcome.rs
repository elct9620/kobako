//! Outcome-buffer classification: bytes → `Value` or `Error`.
//!
//! The SDK twin of the Ruby gem's `Kobako::Outcome` module: split the
//! one-byte tag, decode the branch via `kobako-codec`, and attribute
//! every failure the way the three-layer taxonomy demands — so both
//! frontends looking at the same outcome bytes reach the same error
//! variant. The wire-violation attribution string is the SPEC-pinned
//! wire-level error class name, not a Ruby leakage.

use kobako_codec::codec::{Decoder, Value};
use kobako_codec::outcome::Panic;

use crate::error::{Error, GuestFailure};

/// Outcome-buffer tag for the success branch.
const TAG_VALUE: u8 = 0x01;
/// Outcome-buffer tag for the Panic branch.
const TAG_PANIC: u8 = 0x02;

/// SPEC-pinned wire-level error class, carried as the attribution of
/// host-detected wire violations on both frontends.
const WIRE_ERROR_CLASS: &str = "Kobako::Transport::Error";

/// Classify one OUTCOME_BUFFER: the decoded return value, or the
/// `Error` variant its failure attributes to.
pub(crate) fn decode(bytes: &[u8]) -> Result<Value, Error> {
    let Some((&tag, body)) = bytes.split_first() else {
        return Err(Error::Trap(
            "Sandbox exited without producing a result".into(),
        ));
    };
    match tag {
        TAG_VALUE => decode_value(body),
        TAG_PANIC => Err(decode_panic(body)),
        _ => Err(Error::Trap(
            "Sandbox produced an unrecognised result; the runtime is corrupted, \
             discard this Sandbox before another invocation"
                .into(),
        )),
    }
}

/// Success branch: a decode fault means the framing was fine but the
/// carried value violates the wire — a sandbox-origin fault, with the
/// codec detail preserved for operator triage.
fn decode_value(body: &[u8]) -> Result<Value, Error> {
    let mut decoder = Decoder::new(body);
    decoder
        .read_only_value()
        .map_err(|err| wire_violation("Sandbox produced an invalid result value", &err))
}

/// Panic branch: a well-formed record maps onto the taxonomy by its
/// origin and class; a malformed record is itself a wire violation.
fn decode_panic(body: &[u8]) -> Error {
    match <Panic as kobako_codec::codec::Decode>::decode(body) {
        Ok(panic) => classify_panic(panic),
        Err(err) => wire_violation("Sandbox produced an invalid panic record", &err),
    }
}

/// `origin == "service"` → `Service`; a sandbox-origin panic carrying
/// the bytecode rejection class → `Bytecode`; everything else →
/// `Sandbox`.
fn classify_panic(panic: Panic) -> Error {
    let failure = GuestFailure {
        class: panic.class,
        message: panic.message,
        backtrace: panic.backtrace,
        details: panic.details,
    };
    if panic.origin == "service" {
        Error::Service(failure)
    } else if failure.class == "Kobako::BytecodeError" {
        Error::Bytecode(failure)
    } else {
        Error::Sandbox(failure)
    }
}

fn wire_violation(message: &str, detail: &kobako_codec::codec::Error) -> Error {
    Error::Sandbox(GuestFailure {
        class: WIRE_ERROR_CLASS.into(),
        message: message.into(),
        backtrace: Vec::new(),
        details: Some(Value::Str(detail.to_string())),
    })
}

#[cfg(test)]
mod tests {
    use kobako_codec::codec::Encode;
    use kobako_codec::outcome::Outcome;

    use super::*;

    fn panic_bytes(origin: &str, class: &str) -> Vec<u8> {
        Outcome::Panic(Panic {
            origin: origin.into(),
            class: class.into(),
            message: "boom".into(),
            backtrace: vec![],
            details: None,
        })
        .encode()
        .unwrap()
    }

    #[test]
    fn value_branch_decodes_to_the_carried_value() {
        let bytes = Outcome::Value(Value::Int(42)).encode().unwrap();
        assert_eq!(decode(&bytes).unwrap(), Value::Int(42));
    }

    #[test]
    fn service_origin_panic_becomes_service_error() {
        let result = decode(&panic_bytes("service", "Kobako::ServiceError"));
        assert!(matches!(result, Err(Error::Service(f)) if f.message == "boom"));
    }

    #[test]
    fn bytecode_class_panic_becomes_bytecode_error() {
        let result = decode(&panic_bytes("sandbox", "Kobako::BytecodeError"));
        assert!(matches!(result, Err(Error::Bytecode(_))));
    }

    #[test]
    fn sandbox_origin_panic_becomes_sandbox_error() {
        let result = decode(&panic_bytes("sandbox", "RuntimeError"));
        assert!(matches!(result, Err(Error::Sandbox(f)) if f.class == "RuntimeError"));
    }

    #[test]
    fn empty_bytes_walk_the_trap_path() {
        assert!(matches!(decode(&[]), Err(Error::Trap(_))));
    }

    #[test]
    fn unknown_tag_walks_the_trap_path() {
        assert!(matches!(decode(&[0x7f, 0x2a]), Err(Error::Trap(_))));
    }

    #[test]
    fn malformed_value_body_is_a_wire_violation_sandbox_error() {
        // Tag 0x01 followed by a truncated msgpack str header.
        let result = decode(&[TAG_VALUE, 0xd9]);
        assert!(matches!(result, Err(Error::Sandbox(f)) if f.class == WIRE_ERROR_CLASS));
    }

    #[test]
    fn malformed_panic_body_is_a_wire_violation_sandbox_error() {
        // Tag 0x02 followed by a non-map payload.
        let mut bad = vec![TAG_PANIC];
        bad.push(0x2a);
        let result = decode(&bad);
        assert!(matches!(result, Err(Error::Sandbox(f)) if f.class == WIRE_ERROR_CLASS));
    }
}
