//! Outcome-buffer classification: bytes → payload bytes or `Error`.
//!
//! The SDK twin of the Ruby gem's `Kobako::Outcome` module: split the
//! one-byte tag and attribute every failure the way the three-layer
//! taxonomy demands — so both frontends looking at the same outcome
//! bytes reach the same error variant. Attribution reads the envelope
//! alone; the Result arm's payload meets a schema later, on the
//! `Execution` accessor that asks for one. The wire-violation attribution string is the SPEC-pinned
//! wire-level error class name, not a Ruby leakage.

use kobako_transport::envelope::{Outcome, Panic};

use crate::error::{Error, Failure};

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

#[cfg(test)]
mod tests {
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

    #[test]
    fn the_result_arm_yields_the_payload_bytes_it_carried() {
        assert_eq!(
            classify(&Outcome::Result(vec![0x2a]).encode()).unwrap(),
            vec![0x2a],
            "a Result arm through classify must hand back its payload bytes untouched, \
             since attribution reads no payload byte"
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
