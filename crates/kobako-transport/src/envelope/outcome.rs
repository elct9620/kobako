//! The Outcome envelope — one invocation's final result.
//!
//! Every Panic field is typed here, so a host attributes a failed
//! invocation and reports the correction for it without decoding a payload
//! byte. Only the Result arm carries codec-encoded bytes.

use super::bytes::{Reader, Writer};
use super::{DecodeError, ErrorRecord};

const TAG_RESULT: u8 = 0x01;
const TAG_PANIC: u8 = 0x02;

/// Who a failed invocation attributes to.
///
/// The wire field is an open set of byte strings, but only two values
/// carry meaning — so both sides read and write it through this type and
/// cannot come to spell either one differently. The fallback rule lives in
/// `from_name`: an unrecognised value attributes to the sandbox rather
/// than widening what a Service can claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Origin {
    /// The guest script raised, or a boot step faulted.
    #[default]
    Sandbox,
    /// An unrescued Service call raised.
    Service,
}

impl Origin {
    /// This attribution's spelling on the wire.
    pub fn name(self) -> &'static str {
        match self {
            Origin::Sandbox => "sandbox",
            Origin::Service => "service",
        }
    }

    /// Read a wire spelling back. Everything other than `"service"`,
    /// recognised or not, attributes to the sandbox.
    pub fn from_name(name: &str) -> Self {
        match name {
            "service" => Origin::Service,
            _ => Origin::Sandbox,
        }
    }
}

/// How an invocation ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The invocation completed; the bytes are its value, codec-encoded.
    Result(Vec<u8>),
    /// The invocation terminated with an uncaught exception.
    Panic(Panic),
}

/// An uncaught top-level failure, plus what attribution and correction need.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Panic {
    pub origin: Origin,
    pub error: ErrorRecord,
    /// The names the invocation could have used in place of the one it
    /// named. Empty when the failure offers no correction.
    pub available: Vec<String>,
}

impl Outcome {
    pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(bytes);
        match reader.u8()? {
            TAG_RESULT => Ok(Outcome::Result(reader.remaining().to_vec())),
            TAG_PANIC => {
                let origin = Origin::from_name(reader.text()?);
                let error = ErrorRecord::read(&mut reader)?;
                let available = reader.text_list()?;
                reader.finish()?;
                Ok(Outcome::Panic(Panic {
                    origin,
                    error,
                    available,
                }))
            }
            _ => Err(DecodeError::new(
                "Outcome tag must be 0x01 (result) or 0x02 (panic)",
            )),
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut writer = Writer::new();
        match self {
            Outcome::Result(value) => {
                writer.u8(TAG_RESULT).remainder(value);
            }
            Outcome::Panic(panic) => {
                writer.u8(TAG_PANIC).bytes(panic.origin.name().as_bytes());
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
            origin: Origin::Sandbox,
            error: ErrorRecord {
                name: "RuntimeError".into(),
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
            origin: Origin::Service,
            ..panic_sample()
        };
        assert_eq!(
            Outcome::decode(&Outcome::Panic(service.clone()).encode()),
            Ok(Outcome::Panic(service)),
            "a Panic written with Service attribution must decode back to it, since the \
             origin field is all a host reads to attribute"
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
        // Written by hand: the origin field is an open set on the wire, so
        // this is the shape a third-party guest can legally produce and the
        // one the fallback rule exists for.
        let mut writer = Writer::new();
        writer.u8(TAG_PANIC).bytes(b"something-else");
        panic_sample().error.write(&mut writer);
        writer.list::<&[u8]>(&[]);
        assert_eq!(
            Outcome::decode(&writer.into_bytes()),
            Ok(Outcome::Panic(panic_sample())),
            "an origin outside the reserved set must decode as sandbox attribution rather \
             than widening what a Service can claim"
        );
    }

    #[test]
    fn golden_layout_pins_the_result_tag() {
        assert_eq!(
            Outcome::Result(vec![0x2a]).encode(),
            vec![0x01, 0x2a],
            "a Result must encode as tag byte 0x01 followed by the value alone"
        );
    }

    #[test]
    fn golden_layout_pins_the_panic_field_order() {
        let panic = Panic {
            origin: Origin::Service,
            error: ErrorRecord {
                name: "E".into(),
                message: "m".into(),
                backtrace: vec!["l".into()],
            },
            available: vec!["W".into()],
        };
        assert_eq!(
            Outcome::Panic(panic).encode(),
            vec![
                0x02, // tag: panic
                0, 0, 0, 7, b's', b'e', b'r', b'v', b'i', b'c', b'e', // origin
                0, 0, 0, 1, b'E', // name
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
