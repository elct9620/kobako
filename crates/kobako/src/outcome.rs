//! Outcome-buffer classification: bytes → `Value` or `Error`.
//!
//! The SDK twin of the Ruby gem's `Kobako::Outcome` module: split the
//! one-byte tag, decode the branch via `kobako-codec`, and attribute
//! every failure the way the three-layer taxonomy demands — so both
//! frontends looking at the same outcome bytes reach the same error
//! variant. The wire-violation attribution string is the SPEC-pinned
//! wire-level error class name, not a Ruby leakage.

use kobako_codec::codec::{Decoder, Value};
use kobako_runtime::envelope::{Outcome, Panic};

use crate::error::{Error, GuestFailure};

/// SPEC-pinned wire-level error class, carried as the attribution of
/// host-detected wire violations on both frontends.
const WIRE_ERROR_CLASS: &str = "Kobako::Transport::Error";

/// Classify one OUTCOME_BUFFER: the decoded return value, or the
/// `Error` variant its failure attributes to.
pub(crate) fn decode(bytes: &[u8]) -> Result<Value, Error> {
    match Outcome::decode(bytes) {
        Ok(Outcome::Result(payload)) => decode_value(&payload),
        Ok(Outcome::Panic(panic)) => Err(classify_panic(panic)),
        // Framing the outcome is the one thing the host does before
        // attribution, so a message it cannot frame — an empty buffer
        // included — leaves nothing to attribute to.
        Err(_) if bytes.is_empty() => Err(Error::Trap(
            "Sandbox exited without producing a result".into(),
        )),
        Err(_) => Err(Error::Trap(
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
    let value = decoder
        .read_only_value()
        .map_err(|err| wire_violation("Sandbox produced an invalid result value", &err))?;
    // A Result envelope is a payload position: the Fault envelope's only
    // home is a Reply's fault arm, so an ext 0x02 in the carried value is
    // a wire violation.
    if value.contains_errenv() {
        return Err(wire_violation(
            "Sandbox produced an invalid result value",
            &kobako_codec::codec::Error::Malformed(
                "Fault envelope (ext 0x02) is not a legal value in a Result envelope",
            ),
        ));
    }
    Ok(value)
}

/// `origin == "service"` → `Service`; a sandbox-origin panic carrying
/// the bytecode rejection class → `Bytecode`; everything else →
/// `Sandbox`. Details are a payload position, so an ext 0x02 Fault among
/// them is a wire violation — a Panic whose diagnostics violate the wire
/// is not a record worth attributing from.
fn classify_panic(panic: Panic) -> Error {
    let from_service = panic.from_service();
    let details = match decode_details(&panic.details) {
        Ok(details) => details,
        Err(err) => return err,
    };
    let failure = GuestFailure {
        class: panic.error.class,
        message: panic.error.message,
        backtrace: panic.error.backtrace,
        details,
    };
    if from_service {
        Error::Service(failure)
    } else if failure.class == "Kobako::BytecodeError" {
        Error::Bytecode(failure)
    } else {
        Error::Sandbox(failure)
    }
}

/// A Panic's structured diagnostics, or `None` when the arm carried none
/// or this endpoint's adapter could not read what it carried.
/// Attribution comes off the core envelope, so diagnostics it cannot read
/// are dropped rather than replacing a real failure with a report about
/// its supplementary field.
///
/// A Fault (ext 0x02) among them is the one exception: that is a
/// placement violation rather than unreadable bytes, and it takes the
/// invalid-record channel so a guest breaking the rule is not silently
/// tolerated.
fn decode_details(details: &[u8]) -> Result<Option<Value>, Error> {
    if details.is_empty() {
        return Ok(None);
    }
    let Ok(value) = Decoder::new(details).read_only_value() else {
        return Ok(None);
    };
    if value.contains_errenv() {
        return Err(wire_violation(
            "Sandbox produced an invalid panic record",
            &kobako_codec::codec::Error::Malformed(
                "Fault envelope (ext 0x02) is not a legal value in a Panic envelope",
            ),
        ));
    }
    Ok(Some(value))
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
    use kobako_codec::codec::Encoder;
    use kobako_runtime::envelope::ErrorRecord;

    use super::*;

    fn panic_bytes(origin: &str, class: &str) -> Vec<u8> {
        Outcome::Panic(Panic {
            origin: origin.into(),
            error: ErrorRecord {
                class: class.into(),
                message: "boom".into(),
                backtrace: Vec::new(),
            },
            details: Vec::new(),
        })
        .encode()
    }

    fn result_bytes(value: &Value) -> Vec<u8> {
        Outcome::Result(Encoder::encode(value).unwrap()).encode()
    }

    #[test]
    fn value_branch_decodes_to_the_carried_value() {
        assert_eq!(
            decode(&result_bytes(&Value::Int(42))).unwrap(),
            Value::Int(42)
        );
    }

    // E-50: a Result envelope smuggling an ext 0x02 surfaces through the
    // invalid-result wire-violation channel, matching the Ruby frontend.
    #[test]
    fn value_branch_rejects_errenv_as_wire_violation() {
        let result = decode(&result_bytes(&Value::ErrEnv(vec![0x80])));
        assert!(
            matches!(result, Err(Error::Sandbox(ref f)) if f.message.contains("invalid result value")),
            "expected the invalid-result wire violation, got {result:?}"
        );
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
        // The Result arm followed by a truncated msgpack str header.
        let result = decode(&Outcome::Result(vec![0xd9]).encode());
        assert!(matches!(result, Err(Error::Sandbox(f)) if f.class == WIRE_ERROR_CLASS));
    }

    #[test]
    fn a_panic_record_the_envelope_cannot_frame_walks_the_trap_path() {
        // The Panic arm followed by a truncated origin length prefix.
        let result = decode(&[0x02, 0x00, 0x00]);
        assert!(
            matches!(result, Err(Error::Trap(_))),
            "a Panic the envelope cannot frame leaves nothing to attribute to, got {result:?}"
        );
    }

    // B-66: attribution comes off the core envelope, so diagnostics this
    // endpoint's adapter cannot read are dropped rather than replacing the
    // failure with a report about its supplementary field.
    #[test]
    fn panic_details_the_adapter_cannot_read_are_dropped() {
        let bytes = Outcome::Panic(Panic {
            origin: "service".into(),
            error: ErrorRecord {
                class: "Kobako::ServiceError".into(),
                message: "connection refused".into(),
                backtrace: Vec::new(),
            },
            details: vec![0xc1],
        })
        .encode();
        let result = decode(&bytes);
        assert!(
            matches!(result, Err(Error::Service(ref f)) if f.message == "connection refused" && f.details.is_none()),
            "a Service failure must keep attributing to the Service with its own message, got {result:?}"
        );
    }

    // E-50: a Panic smuggling an ext 0x02 in its details surfaces
    // through the invalid-record channel, matching the Ruby frontend.
    #[test]
    fn panic_details_carrying_errenv_are_a_wire_violation() {
        let bytes = Outcome::Panic(Panic {
            origin: "sandbox".into(),
            error: ErrorRecord {
                class: "RuntimeError".into(),
                message: "boom".into(),
                backtrace: Vec::new(),
            },
            details: Encoder::encode(&Value::ErrEnv(vec![0x80])).unwrap(),
        })
        .encode();
        let result = decode(&bytes);
        assert!(
            matches!(result, Err(Error::Sandbox(ref f)) if f.message.contains("invalid panic record")),
            "expected the invalid-panic-record wire violation, got {result:?}"
        );
    }
}
