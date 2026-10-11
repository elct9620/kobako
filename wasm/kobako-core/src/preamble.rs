//! The Preamble an invocation reads from stdin before it runs anything.
//!
//! It arrives as a 4-byte big-endian u32 length plus its bytes
//! (docs/wire/abi.md § Invocation inputs). Reading and decoding it here
//! means a flow starts from the decoded Preamble, never from stdin; one
//! that does not read or decode answers the invocation as a boot failure
//! before any flow runs.

use std::io::Read;

use kobako_transport::abi::{FRAME_LEN_SIZE, MAX_FRAME_LEN};
use kobako_transport::envelope::{ErrorRecord, Origin, Outcome, Panic, Preamble};

/// The Preamble every invocation reads before its entry runs.
pub(crate) fn read_preamble() -> Result<Preamble, Outcome> {
    read_preamble_from(&mut std::io::stdin().lock())
}

fn read_preamble_from<R: Read>(input: &mut R) -> Result<Preamble, Outcome> {
    let bytes = read_framed(input)
        .ok_or_else(|| boot_failure("failed to read the Sandbox's Preamble".into()))?;
    Preamble::decode(&bytes)
        .map_err(|err| boot_failure(format!("failed to decode the Sandbox's Preamble: {err}")))
}

/// The failure a guest reports when it cannot start the Sandbox, under
/// the class the host raises for it.
fn boot_failure(message: String) -> Outcome {
    Outcome::Panic(Panic {
        origin: Origin::Sandbox,
        error: ErrorRecord {
            name: "Kobako::BootError".into(),
            message,
            backtrace: Vec::new(),
        },
        available: Vec::new(),
    })
}

/// `None` on EOF, a short read, or a length past the cap, so a hostile
/// prefix never sizes an allocation.
fn read_framed<R: Read>(input: &mut R) -> Option<Vec<u8>> {
    let mut len_buf = [0u8; FRAME_LEN_SIZE];
    input.read_exact(&mut len_buf).ok()?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > MAX_FRAME_LEN {
        return None;
    }
    let mut payload = vec![0u8; len];
    input.read_exact(&mut payload).ok()?;
    Some(payload)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kobako_transport::envelope::Bindings;

    fn framed(payload: &[u8]) -> std::io::Cursor<Vec<u8>> {
        let mut bytes = (payload.len() as u32).to_be_bytes().to_vec();
        bytes.extend_from_slice(payload);
        std::io::Cursor::new(bytes)
    }

    fn boot_message(outcome: Outcome) -> String {
        let Outcome::Panic(panic) = outcome else {
            panic!("a Preamble that fails must answer with a Panic");
        };
        assert_eq!(
            (panic.origin, panic.error.name.as_str()),
            (Origin::Sandbox, "Kobako::BootError"),
            "a Preamble that fails through read_preamble must answer as the Sandbox's boot \
             failure"
        );
        panic.error.message
    }

    // @behavior WE-066
    #[test]
    fn read_framed_round_trips_a_prefixed_payload() {
        let mut cursor = framed(b"hello");
        assert_eq!(
            read_framed(&mut cursor),
            Some(b"hello".to_vec()),
            "a length-prefixed payload through read_framed must yield its bytes unchanged"
        );
    }

    // @behavior WE-067
    #[test]
    fn read_framed_rejects_an_over_cap_length_prefix() {
        let mut framed = ((MAX_FRAME_LEN as u32) + 1).to_be_bytes().to_vec();
        framed.extend_from_slice(b"x");
        let mut cursor = std::io::Cursor::new(framed);
        assert_eq!(
            read_framed(&mut cursor),
            None,
            "a length prefix past the allocation ceiling through read_framed must be refused \
             before anything is allocated for it"
        );
    }

    #[test]
    fn stdin_yields_the_decoded_preamble() {
        let mut preamble = Preamble::default();
        preamble.bindings = Bindings {
            paths: vec!["A".into()],
        };
        let mut input = framed(&preamble.encode());

        assert_eq!(
            read_preamble_from(&mut input),
            Ok(preamble),
            "stdin through read_preamble must yield the Preamble the host encoded"
        );
    }

    // @behavior OC-043
    #[test]
    fn a_missing_preamble_answers_as_a_boot_failure() {
        let mut input = std::io::Cursor::new(Vec::new());

        assert_eq!(
            boot_message(read_preamble_from(&mut input).unwrap_err()),
            "failed to read the Sandbox's Preamble",
            "stdin that ends before the Preamble through read_preamble must answer the \
             invocation as a boot failure"
        );
    }

    // @behavior OC-043
    #[test]
    fn an_undecodable_preamble_answers_as_a_boot_failure() {
        let mut input = framed(&[0xff]);

        assert!(
            boot_message(read_preamble_from(&mut input).unwrap_err())
                .starts_with("failed to decode the Sandbox's Preamble"),
            "a Preamble that does not decode through read_preamble must answer the \
             invocation as a boot failure"
        );
    }

    // @behavior WE-078
    #[test]
    fn a_critical_record_this_guest_does_not_know_answers_as_a_boot_failure() {
        // One critical record of the unassigned kind 0x7e, with an empty body.
        let mut input = framed(&[0, 0, 0, 1, 1, 0x7e, 0, 0, 0, 0]);

        assert!(
            boot_message(read_preamble_from(&mut input).unwrap_err()).contains("critical record"),
            "a critical record of a kind this guest does not know through read_preamble must \
             refuse the invocation as a boot failure naming why"
        );
    }
}
