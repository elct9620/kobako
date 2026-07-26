//! The Outcome envelope — one invocation's final result.
//!
//! Every Panic field is typed here, so a host attributes a failed
//! invocation and reports the correction for it without decoding a payload
//! byte. Only the Result arm carries adapter-encoded bytes.

use super::bytes::{Reader, Writer};
use super::{Error, ErrorRecord};

const TAG_RESULT: u8 = 0x01;
const TAG_PANIC: u8 = 0x02;

/// The `origin` value that attributes a failure to an unrescued Service
/// call. Every other value, recognised or not, attributes to the sandbox.
/// Callers read the answer through `Panic::from_service` rather than
/// comparing the string themselves.
const ORIGIN_SERVICE: &str = "service";

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

impl Panic {
    /// Whether this failure attributes to an unrescued Service call rather
    /// than to the guest script.
    pub fn from_service(&self) -> bool {
        self.origin == ORIGIN_SERVICE
    }
}

impl Outcome {
    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        let mut reader = Reader::new(bytes);
        match reader.u8()? {
            TAG_RESULT => Ok(Outcome::Result(reader.remaining().to_vec())),
            TAG_PANIC => {
                let origin = reader.text()?.to_owned();
                let error = ErrorRecord::read(&mut reader)?;
                let available = reader.text_list()?;
                reader.finish()?;
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
        let mut writer = Writer::new();
        match self {
            Outcome::Result(value) => {
                writer.u8(TAG_RESULT).remainder(value);
            }
            Outcome::Panic(panic) => {
                writer.u8(TAG_PANIC).bytes(panic.origin.as_bytes());
                panic.error.write(&mut writer);
                writer.list(&panic.available);
            }
        }
        writer.into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn panic_sample() -> Panic {
        Panic {
            origin: "sandbox".into(),
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
            "a Result Outcome must survive a host encode and decode unchanged"
        );
    }

    #[test]
    fn a_panic_round_trips() {
        let outcome = Outcome::Panic(panic_sample());
        let encoded = outcome.encode();
        assert_eq!(
            Outcome::decode(&encoded),
            Ok(outcome),
            "a Panic Outcome must survive a host encode and decode unchanged"
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
    fn attribution_reads_origin_alone() {
        let service = Panic {
            origin: ORIGIN_SERVICE.into(),
            ..panic_sample()
        };
        assert!(
            service.from_service(),
            "an origin of \"service\" must attribute to the Service"
        );
        assert!(
            !panic_sample().from_service(),
            "any other origin must attribute to the sandbox"
        );
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
    fn an_unrecognised_origin_attributes_to_the_sandbox() {
        let panic = Panic {
            origin: "something-else".into(),
            ..panic_sample()
        };
        assert!(
            !panic.from_service(),
            "an origin outside the reserved set must fall back to sandbox attribution"
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
    fn golden_layout_pins_the_panic_field_order() {
        let panic = Panic {
            origin: ORIGIN_SERVICE.into(),
            error: ErrorRecord {
                class: "E".into(),
                message: "m".into(),
                backtrace: vec!["l".into()],
            },
            available: vec!["W".into()],
        };
        assert_eq!(
            Outcome::Panic(panic).encode(),
            vec![
                TAG_PANIC, //
                0, 0, 0, 7, b's', b'e', b'r', b'v', b'i', b'c', b'e', // origin
                0, 0, 0, 1, b'E', // class
                0, 0, 0, 1, b'm', // message
                0, 0, 0, 1, // backtrace count
                0, 0, 0, 1, b'l', // backtrace[0]
                0, 0, 0, 1, // available count
                0, 0, 0, 1, b'W', // available[0]
            ],
            "attribution reads origin before the Error Record, so the field order must stay fixed"
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
