//! The Reply envelope and its reverse-direction sibling, the Yield Reply.
//!
//! Success-versus-fault is decided by a tag byte rather than a reserved
//! payload value, so a side learns the outcome of a Call whatever schema
//! the payload carries. Each tag is the answer's arm in the shared
//! registry, so the same byte means the same arm in every answer.

use super::bytes::{Reader, Writer};
use super::{tag, DecodeError, ErrorRecord, Fault, Unsupported};

/// The answer to one dispatch Call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    /// The method returned; the bytes are the value, codec-encoded.
    Ok(Vec<u8>),
    /// The method refused or failed. Typed rather than codec-encoded:
    /// every field of a Fault is kobako's, so a guest reads a refusal
    /// whatever schema it speaks.
    Fault(Fault),
    /// A block exit unwound the method; the guest continues that exit,
    /// which it kept, so nothing of it crosses.
    Break,
    /// The receiver does not serve the request it was sent.
    Unsupported(Unsupported),
}

impl Reply {
    pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(bytes);
        match reader.u8()? {
            tag::OK => Ok(Reply::Ok(reader.remaining().to_vec())),
            // No `finish` here: a Fault owns everything after the tag,
            // including the fields a later contract version appends.
            tag::FAULT => Ok(Reply::Fault(Fault::read(&mut reader)?)),
            tag::BREAK => {
                reader.finish()?;
                Ok(Reply::Break)
            }
            tag::UNSUPPORTED => Ok(Reply::Unsupported(Unsupported::read(&mut reader)?)),
            _ => Err(DecodeError::new(
                "Reply tag must be 0x10 (ok), 0x11 (fault), 0x12 (break), or 0x15 (unsupported)",
            )),
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut writer = Writer::new();
        match self {
            Reply::Ok(body) => {
                writer.u8(tag::OK).remainder(body);
            }
            Reply::Fault(fault) => {
                writer.u8(tag::FAULT);
                fault.write(&mut writer);
            }
            Reply::Break => {
                writer.u8(tag::BREAK);
            }
            Reply::Unsupported(refusal) => return refusal.encode(),
        }
        writer.into_bytes()
    }
}

/// The answer to one Yield Call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum YieldReply {
    /// The block completed; the bytes are its value, codec-encoded.
    Ok(Vec<u8>),
    /// The block ran `break` or `return` to a frame past the Service.
    /// The guest keeps the exit, so the Service is only unwound.
    Break,
    /// The block raised, or failed in a way the host re-raises at the
    /// yield site.
    Error(ErrorRecord),
    /// The guest serves no block.
    Unsupported(Unsupported),
}

impl YieldReply {
    pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(bytes);
        match reader.u8()? {
            tag::OK => Ok(YieldReply::Ok(reader.remaining().to_vec())),
            tag::BREAK => {
                reader.finish()?;
                Ok(YieldReply::Break)
            }
            tag::ERROR => {
                let record = ErrorRecord::read(&mut reader)?;
                reader.finish()?;
                Ok(YieldReply::Error(record))
            }
            tag::UNSUPPORTED => Ok(YieldReply::Unsupported(Unsupported::read(&mut reader)?)),
            _ => Err(DecodeError::new("Yield Reply tag is not recognised")),
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut writer = Writer::new();
        match self {
            YieldReply::Ok(body) => {
                writer.u8(tag::OK).remainder(body);
            }
            YieldReply::Break => {
                writer.u8(tag::BREAK);
            }
            YieldReply::Error(record) => {
                writer.u8(tag::ERROR);
                record.write(&mut writer);
            }
            YieldReply::Unsupported(refusal) => return refusal.encode(),
        }
        writer.into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // @behavior WE-018
    #[test]
    fn every_reply_arm_round_trips() {
        for reply in [
            Reply::Ok(vec![0x2a]),
            Reply::Fault(Fault::new(
                super::super::FaultKind::Undefined,
                "no such method",
            )),
            Reply::Break,
            Reply::Unsupported(Unsupported::new("no such operation")),
        ] {
            let encoded = reply.encode();
            assert_eq!(
                Reply::decode(&encoded),
                Ok(reply),
                "a Reply must survive a host encode and decode on every arm"
            );
        }
    }

    // @behavior WE-019
    #[test]
    fn an_empty_ok_body_round_trips() {
        let encoded = Reply::Ok(Vec::new()).encode();
        assert_eq!(
            Reply::decode(&encoded),
            Ok(Reply::Ok(Vec::new())),
            "a Reply whose payload is empty must decode as an empty value, not a truncation"
        );
    }

    // @behavior WE-020
    #[test]
    fn an_unknown_reply_tag_is_refused() {
        assert!(
            Reply::decode(&[0x13, 0]).is_err(),
            "a Reply tag outside its arms must be rejected as a wire violation"
        );
    }

    // @behavior WE-021
    #[test]
    fn a_zero_length_reply_is_refused() {
        assert!(
            Reply::decode(&[]).is_err(),
            "a zero-length Reply must be rejected rather than read as a missing tag"
        );
    }

    // @behavior WE-022
    #[test]
    fn every_live_yield_arm_round_trips() {
        let arms = [
            YieldReply::Ok(vec![0xa4]),
            YieldReply::Break,
            YieldReply::Error(ErrorRecord {
                name: "LocalJumpError".into(),
                message: "no block".into(),
                backtrace: vec!["(eval):1".into()],
            }),
            YieldReply::Unsupported(Unsupported::new("no block support")),
        ];
        for arm in arms {
            let encoded = arm.encode();
            assert_eq!(
                YieldReply::decode(&encoded),
                Ok(arm),
                "every live Yield Reply arm must survive a host encode and decode"
            );
        }
    }

    // @behavior WE-024
    #[test]
    fn an_unknown_yield_tag_is_refused() {
        for tag in [0x03, 0x11, 0x7f] {
            assert!(
                YieldReply::decode(&[tag]).is_err(),
                "a Yield Reply tag {tag:#04x} outside the live set must be rejected as a \
                 wire violation"
            );
        }
    }

    // @behavior WE-025
    #[test]
    fn golden_layout_pins_the_reply_tags() {
        // The same body on both arms: what separates "the Service returned
        // this" from "the Service failed with this" is the tag alone, which
        // is what lets a side read the outcome without a payload codec.
        assert_eq!(
            Reply::Ok(vec![0x2a]).encode(),
            vec![0x10, 0x2a],
            "a successful Reply must encode as tag byte 0x10 followed by the body alone"
        );
        assert_eq!(
            Reply::Fault(Fault::new(super::super::FaultKind::Runtime, "x")).encode(),
            vec![0x11, 0, 0, 0, 0, 1, b'x'],
            "a fault Reply must encode as tag byte 0x11 followed by the Fault's own fixed \
             layout, so a guest reads a refusal without reaching a payload codec"
        );
        assert_eq!(
            Reply::Unsupported(Unsupported::new("x")).encode(),
            vec![0x15, 0, 0, 0, 0, 1, b'x'],
            "an unsupported Reply must encode as tag byte 0x15 followed by kind 0 and the \
             message"
        );
        assert_eq!(
            Reply::Break.encode(),
            vec![0x12],
            "a break Reply must encode as tag byte 0x12 alone, since the guest keeps the exit"
        );
    }

    // @behavior WE-068
    #[test]
    fn every_answer_takes_its_ok_tag_from_one_registry() {
        let body = vec![0x2a];
        assert_eq!(
            [
                Reply::Ok(body.clone()).encode()[0],
                YieldReply::Ok(body.clone()).encode()[0],
                super::super::CompileReply::Ok(vec![body.clone()]).encode()[0],
                super::super::Outcome::Ok(body).encode()[0],
            ],
            [0x10, 0x10, 0x10, 0x10],
            "a Reply, a Yield Reply, a Compile Reply and an Outcome on their ok arm must all \
             open with tag byte 0x10"
        );
    }

    // @behavior WE-026
    #[test]
    fn golden_layout_pins_the_yield_tags() {
        assert_eq!(
            YieldReply::Ok(vec![0xc0]).encode(),
            vec![0x10, 0xc0],
            "a Yield Reply carrying the block's value must encode as tag byte 0x10"
        );
        assert_eq!(
            YieldReply::Break.encode(),
            vec![0x12],
            "a Yield Reply for a block exit must encode as tag byte 0x12 alone"
        );
        assert_eq!(
            YieldReply::Unsupported(Unsupported::new("x")).encode(),
            vec![0x15, 0, 0, 0, 0, 1, b'x'],
            "a Yield Reply from a guest serving no block must encode as tag byte 0x15"
        );
        assert_eq!(
            YieldReply::Error(ErrorRecord {
                name: "E".into(),
                message: "m".into(),
                backtrace: Vec::new(),
            })
            .encode(),
            vec![
                0x13, // tag: error
                0, 0, 0, 1, b'E', // name
                0, 0, 0, 1, b'm', // message
                0, 0, 0, 0, // backtrace count
            ],
            "a Yield Reply carrying a guest failure must encode as tag byte 0x13 followed \
             by an Error Record rather than a codec-encoded body"
        );
    }
}
