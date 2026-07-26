//! The Outcome envelope — guest side.
//!
//! The guest writes exactly one of these per invocation before its entry
//! export returns. Every Panic field is typed here, so only the Result arm
//! asks the payload adapter for anything.

use super::codec::{expect_end, put_bytes, put_list, rest, take_text, take_text_list, take_u8};
use super::{Error, ErrorRecord};

const TAG_RESULT: u8 = 0x01;
const TAG_PANIC: u8 = 0x02;

/// The `origin` value that attributes a failure to an unrescued Service
/// call. Every other value, recognised or not, attributes to the sandbox.
pub const ORIGIN_SERVICE: &str = "service";
/// The `origin` value for a guest script error or boot fault.
pub const ORIGIN_SANDBOX: &str = "sandbox";

/// How an invocation ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The invocation completed; the bytes are its value, adapter-encoded.
    Result(Vec<u8>),
    /// The invocation terminated with an uncaught exception.
    Panic(Panic),
}

/// An uncaught top-level failure, plus what attribution and correction need.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Panic {
    pub origin: String,
    pub error: ErrorRecord,
    /// The names the invocation could have used in place of the one it
    /// named. Empty when the failure offers no correction.
    pub available: Vec<String>,
}

impl Outcome {
    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        let mut at = 0usize;
        match take_u8(bytes, &mut at)? {
            TAG_RESULT => Ok(Outcome::Result(rest(bytes, &at).to_vec())),
            TAG_PANIC => {
                let origin = take_text(bytes, &mut at)?.to_owned();
                let error = ErrorRecord::take(bytes, &mut at)?;
                let available = take_text_list(bytes, &mut at)?;
                expect_end(bytes, &at)?;
                Ok(Outcome::Panic(Panic {
                    origin,
                    error,
                    available,
                }))
            }
            _ => Err(Error("Outcome tag must be 0x01 (result) or 0x02 (panic)")),
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        match self {
            Outcome::Result(value) => {
                out.push(TAG_RESULT);
                out.extend_from_slice(value);
            }
            Outcome::Panic(panic) => {
                out.push(TAG_PANIC);
                put_bytes(&mut out, panic.origin.as_bytes());
                panic.error.put(&mut out);
                put_list(&mut out, &panic.available);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn panic_sample() -> Panic {
        Panic {
            origin: ORIGIN_SANDBOX.into(),
            error: ErrorRecord {
                class: "RuntimeError".into(),
                message: "boom".into(),
                backtrace: vec!["(eval):1".into()],
            },
            available: Vec::new(),
        }
    }

    #[test]
    fn a_result_round_trips() {
        let outcome = Outcome::Result(vec![0x2a]);
        let encoded = outcome.encode();
        assert_eq!(
            Outcome::decode(&encoded),
            Ok(outcome),
            "a Result Outcome must survive a guest encode and decode unchanged"
        );
    }

    #[test]
    fn a_panic_round_trips() {
        let outcome = Outcome::Panic(panic_sample());
        let encoded = outcome.encode();
        assert_eq!(
            Outcome::decode(&encoded),
            Ok(outcome),
            "a Panic Outcome must survive a guest encode and decode unchanged"
        );
    }

    #[test]
    fn a_panic_carrying_available_names_round_trips() {
        let outcome = Outcome::Panic(Panic {
            available: vec!["Worker".into(), "Helper".into()],
            ..panic_sample()
        });
        let encoded = outcome.encode();
        assert_eq!(
            Outcome::decode(&encoded),
            Ok(outcome),
            "a Panic offering a correction must carry its names through in order"
        );
    }

    #[test]
    fn a_panic_offering_no_correction_decodes_as_an_empty_list() {
        let encoded = Outcome::Panic(panic_sample()).encode();
        match Outcome::decode(&encoded) {
            Ok(Outcome::Panic(panic)) => assert!(
                panic.available.is_empty(),
                "a Panic with no correction to offer must decode as an empty list, not a decode error"
            ),
            other => panic!("expected a Panic, got {other:?}"),
        }
    }

    #[test]
    fn bytes_past_the_available_list_are_refused() {
        let mut encoded = Outcome::Panic(panic_sample()).encode();
        encoded.push(0x2a);
        assert!(
            Outcome::decode(&encoded).is_err(),
            "a Panic is self-delimiting to its last field, so trailing bytes must fail as a framing desync"
        );
    }

    #[test]
    fn golden_layout_pins_the_result_tag() {
        assert_eq!(
            Outcome::Result(vec![0x2a]).encode(),
            vec![TAG_RESULT, 0x2a],
            "a Result must be the tag byte followed by the value alone"
        );
    }

    #[test]
    fn origin_precedes_the_error_record() {
        let encoded = Outcome::Panic(Panic {
            origin: ORIGIN_SERVICE.into(),
            error: ErrorRecord {
                class: "E".into(),
                message: "m".into(),
                backtrace: Vec::new(),
            },
            available: vec!["W".into()],
        })
        .encode();
        assert_eq!(
            encoded,
            vec![
                TAG_PANIC, //
                0, 0, 0, 7, b's', b'e', b'r', b'v', b'i', b'c', b'e', //
                0, 0, 0, 1, b'E', //
                0, 0, 0, 1, b'm', //
                0, 0, 0, 0, // backtrace count
                0, 0, 0, 1, // available count
                0, 0, 0, 1, b'W', //
            ],
            "attribution must read origin before any Error Record field, so the layout pins it first"
        );
    }

    #[test]
    fn a_zero_length_outcome_is_refused() {
        assert!(
            Outcome::decode(&[]).is_err(),
            "a zero-length outcome buffer must be rejected as a wire violation"
        );
    }

    #[test]
    fn an_unknown_outcome_tag_is_refused() {
        assert!(
            Outcome::decode(&[0x03, 0x00]).is_err(),
            "an Outcome tag that is neither result nor panic must be rejected as a wire violation"
        );
    }
}
