//! The Preamble an invocation takes from the host before it runs anything.
//!
//! It arrives through the host's `preamble` import, sent even with no
//! records. Decoding it here means a flow starts from the decoded Preamble;
//! one that does not decode answers the invocation as a boot failure before
//! any flow runs.

use kobako_transport::envelope::{Panic, Preamble, BOOT_ERROR};

use crate::sandbox_panic;

/// The Preamble every invocation takes before its entry runs.
pub(crate) fn read_preamble() -> Result<Preamble, Panic> {
    decode_preamble(&take_preamble())
}

/// The bytes the host hands over through its `preamble` import.
#[cfg(target_arch = "wasm32")]
fn take_preamble() -> Vec<u8> {
    let mut area = [0u32; 2];
    // SAFETY: the import writes the two u32 of `area`, which this frame
    // owns, and names a list it wrote into this instance.
    unsafe {
        crate::abi::preamble(area.as_mut_ptr() as u32);
        crate::abi::take_list(area)
    }
}

/// Off wasm32 there is no host to ask, so the Preamble is empty bytes,
/// which no Preamble decodes from.
#[cfg(not(target_arch = "wasm32"))]
fn take_preamble() -> Vec<u8> {
    Vec::new()
}

fn decode_preamble(bytes: &[u8]) -> Result<Preamble, Panic> {
    Preamble::decode(bytes)
        .map_err(|err| boot_failure(format!("failed to decode the Sandbox's Preamble: {err}")))
}

/// The failure a guest reports when it cannot start the Sandbox.
fn boot_failure(message: String) -> Panic {
    sandbox_panic(BOOT_ERROR, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kobako_transport::envelope::{Bindings, Origin};

    fn boot_message(panic: Panic) -> String {
        assert_eq!(
            (panic.origin, panic.error.name.as_str()),
            (Origin::Sandbox, "Kobako::BootError"),
            "a Preamble that fails through decode_preamble must answer as the Sandbox's boot \
             failure"
        );
        panic.error.message
    }

    #[test]
    fn the_hosts_bytes_yield_the_decoded_preamble() {
        let mut preamble = Preamble::default();
        preamble.bindings = Bindings {
            paths: vec!["A".into()],
        };

        assert_eq!(
            decode_preamble(&preamble.encode()),
            Ok(preamble),
            "the host's preamble bytes through decode_preamble must yield the Preamble the \
             host encoded"
        );
    }

    // @behavior OC-043
    #[test]
    fn an_undecodable_preamble_answers_as_a_boot_failure() {
        for bytes in [&[][..], &[0xff][..]] {
            assert!(
                boot_message(decode_preamble(bytes).unwrap_err())
                    .starts_with("failed to decode the Sandbox's Preamble"),
                "a Preamble of {bytes:?} through decode_preamble must answer the invocation as \
                 a boot failure"
            );
        }
    }

    // @behavior WE-078
    #[test]
    fn a_critical_record_this_guest_does_not_know_answers_as_a_boot_failure() {
        // One critical record of the unassigned kind 0x7e, with an empty body.
        let bytes = [0, 0, 0, 1, 1, 0x7e, 0, 0, 0, 0];

        assert!(
            boot_message(decode_preamble(&bytes).unwrap_err()).contains("critical record"),
            "a critical record of a kind this guest does not know through decode_preamble must \
             refuse the invocation as a boot failure naming why"
        );
    }
}
