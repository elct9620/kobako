//! The Outcome envelope — one invocation's final result.
//!
//! Attribution reads `origin` at this layer, so a host maps a failed
//! invocation onto its own error classes without decoding a payload byte;
//! `details` stays supplementary rather than something attribution needs.

use super::codec::{Reader, Writer};
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

/// An uncaught top-level failure, plus the two fields attribution needs.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Panic {
    pub origin: String,
    pub error: ErrorRecord,
    /// Structured diagnostics, adapter-encoded. Empty means absent.
    pub details: Vec<u8>,
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
                Ok(Outcome::Panic(Panic {
                    origin,
                    error,
                    details: reader.remaining().to_vec(),
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
                writer.remainder(&panic.details);
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
            details: Vec::new(),
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
    fn a_panic_carrying_details_round_trips() {
        let outcome = Outcome::Panic(Panic {
            details: vec![0x91, 0xa1, b'x'],
            ..panic_sample()
        });
        let encoded = outcome.encode();
        assert_eq!(
            Outcome::decode(&encoded),
            Ok(outcome),
            "a Panic with structured details must carry them through as opaque bytes"
        );
    }

    #[test]
    fn absent_details_decode_as_empty() {
        let encoded = Outcome::Panic(panic_sample()).encode();
        match Outcome::decode(&encoded) {
            Ok(Outcome::Panic(panic)) => assert!(
                panic.details.is_empty(),
                "a Panic with no details must decode as an empty remainder, not a decode error"
            ),
            other => panic!("expected a Panic, got {other:?}"),
        }
    }

    #[test]
    fn attribution_reads_origin_without_touching_details() {
        let service = Panic {
            origin: ORIGIN_SERVICE.into(),
            details: vec![0xc1],
            ..panic_sample()
        };
        assert!(
            service.from_service(),
            "an origin of \"service\" must attribute to the Service without decoding details"
        );
        assert!(
            !panic_sample().from_service(),
            "any other origin must attribute to the sandbox"
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
            details: vec![0x2a],
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
                0x2a, // details remainder
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
