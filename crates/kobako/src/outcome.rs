//! Outcome-buffer classification: bytes → payload bytes or `Error`.
//!
//! The SDK twin of the Ruby gem's `Kobako::Outcome` module: split the
//! one-byte tag and attribute every failure the way the three-layer
//! taxonomy demands — so both frontends looking at the same outcome
//! bytes reach the same error variant. Attribution reads the envelope
//! alone; the Result arm's payload meets a schema later, on the
//! `Execution` accessor that asks for one. The wire-violation attribution string is the SPEC-pinned
//! wire-level error class name, not a Ruby leakage.

use kobako_codec::msgpack::codec::{Decoder, Value};
use kobako_transport::envelope::{Outcome, Panic};

use crate::error::{Error, Failure};

/// SPEC-pinned wire-level error class, carried as the attribution of
/// host-detected wire violations on both frontends.
const WIRE_ERROR_CLASS: &str = "Kobako::Transport::Error";

/// Classify one OUTCOME_BUFFER by its envelope alone: the Result arm's
/// payload bytes, or the `Error` its failure attributes to. Reads no
/// payload byte, so attribution works for a host whose Receivers speak
/// a schema this crate does not know.
pub(crate) fn classify(bytes: &[u8]) -> Result<Vec<u8>, Error> {
    match Outcome::decode(bytes) {
        Ok(Outcome::Result(payload)) => Ok(payload),
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
pub(crate) fn decode_value(body: &[u8]) -> Result<Value, Error> {
    let mut decoder = Decoder::new(body);
    let value = decoder
        .read_only_value()
        .map_err(|err| wire_violation("Sandbox produced an invalid result value", &err))?;
    Ok(value)
}

/// `origin == "service"` → `Service`; a sandbox-origin panic carrying
/// the bytecode rejection class → `Bytecode`; everything else →
/// `Sandbox`. Every field is typed at the envelope, so classifying a
/// Panic reads no payload byte and cannot fail.
fn classify_panic(panic: Panic) -> Error {
    let from_service = panic.from_service();
    let failure = Box::new(Failure {
        class: panic.error.class,
        message: panic.error.message,
        backtrace: panic.error.backtrace,
        available: panic.available,
        diagnostic: None,
    });
    if from_service {
        Error::Service(failure)
    } else if failure.class == "Kobako::BytecodeError" {
        Error::Bytecode(failure)
    } else {
        Error::Sandbox(failure)
    }
}

fn wire_violation(message: &str, detail: &kobako_codec::msgpack::codec::Error) -> Error {
    Error::Sandbox(Box::new(Failure {
        class: WIRE_ERROR_CLASS.into(),
        message: message.into(),
        backtrace: Vec::new(),
        available: Vec::new(),
        diagnostic: Some(detail.to_string()),
    }))
}

#[cfg(test)]
mod tests {
    use kobako_codec::msgpack::codec::Encoder;
    use kobako_transport::envelope::ErrorRecord;

    use super::*;

    fn panic_bytes(origin: &str, class: &str) -> Vec<u8> {
        Outcome::Panic(Panic {
            origin: origin.into(),
            error: ErrorRecord {
                class: class.into(),
                message: "boom".into(),
                backtrace: Vec::new(),
            },
            available: Vec::new(),
        })
        .encode()
    }

    fn result_bytes(value: &Value) -> Vec<u8> {
        Outcome::Result(Encoder::encode(value).unwrap()).encode()
    }

    #[test]
    fn the_result_arm_yields_payload_bytes_the_codec_reads_back() {
        assert_eq!(
            decode_value(&classify(&result_bytes(&Value::Int(42))).unwrap()).unwrap(),
            Value::Int(42)
        );
    }

    #[test]
    fn service_origin_panic_becomes_service_error() {
        let result = classify(&panic_bytes("service", "Kobako::ServiceError"));
        assert!(matches!(result, Err(Error::Service(f)) if f.message == "boom"));
    }

    #[test]
    fn bytecode_class_panic_becomes_bytecode_error() {
        let result = classify(&panic_bytes("sandbox", "Kobako::BytecodeError"));
        assert!(matches!(result, Err(Error::Bytecode(_))));
    }

    #[test]
    fn sandbox_origin_panic_becomes_sandbox_error() {
        let result = classify(&panic_bytes("sandbox", "RuntimeError"));
        assert!(matches!(result, Err(Error::Sandbox(f)) if f.class == "RuntimeError"));
    }

    #[test]
    fn empty_bytes_walk_the_trap_path() {
        assert!(matches!(classify(&[]), Err(Error::Trap(_))));
    }

    #[test]
    fn unknown_tag_walks_the_trap_path() {
        assert!(matches!(classify(&[0x7f, 0x2a]), Err(Error::Trap(_))));
    }

    #[test]
    fn malformed_value_body_is_a_wire_violation_sandbox_error() {
        // The Result arm followed by a truncated msgpack str header.
        let result =
            classify(&Outcome::Result(vec![0xd9]).encode()).and_then(|body| decode_value(&body));
        assert!(matches!(result, Err(Error::Sandbox(f)) if f.class == WIRE_ERROR_CLASS));
    }

    #[test]
    fn a_panic_record_the_envelope_cannot_frame_walks_the_trap_path() {
        // The Panic arm followed by a truncated origin length prefix.
        let result = classify(&[0x02, 0x00, 0x00]);
        assert!(
            matches!(result, Err(Error::Trap(_))),
            "a Panic the envelope cannot frame leaves nothing to attribute to, got {result:?}"
        );
    }

    // E-27: an unresolved entrypoint reaches the caller with the names it
    // could have been, matching what the Ruby frontend exposes as
    // `#available` on its own subclass.
    #[test]
    fn an_unresolved_entrypoint_carries_the_names_it_could_have_been() {
        let bytes = Outcome::Panic(Panic {
            origin: "sandbox".into(),
            error: ErrorRecord {
                class: "Kobako::UndefinedEntrypointError".into(),
                message: "undefined entrypoint: Wrker".into(),
                backtrace: Vec::new(),
            },
            available: vec!["Worker".into(), "Helper".into()],
        })
        .encode();
        let result = classify(&bytes);
        assert!(
            matches!(result, Err(Error::Sandbox(ref f)) if f.available == ["Worker", "Helper"]),
            "an unresolved entrypoint must reach the caller with its correction, got {result:?}"
        );
    }
}
