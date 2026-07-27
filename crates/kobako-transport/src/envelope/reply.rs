//! The Reply envelope and its reverse-direction sibling, the Yield Reply.
//!
//! Success-versus-fault is decided by a tag byte rather than a reserved
//! payload value, so a side learns the outcome of a Call whatever schema
//! the payload carries.

use super::bytes::{Reader, Writer};
use super::{Error, ErrorRecord};

const TAG_OK: u8 = 0;
const TAG_FAULT: u8 = 1;

const YIELD_OK: u8 = 0x01;
const YIELD_BREAK: u8 = 0x02;
const YIELD_RESERVED: u8 = 0x03;
const YIELD_ERROR: u8 = 0x04;

/// The answer to one dispatch Call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    /// The method returned; the bytes are the value, codec-encoded.
    Ok(Vec<u8>),
    /// The method refused or failed; the bytes are the fault,
    /// codec-encoded.
    Fault(Vec<u8>),
}

impl Reply {
    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        let mut reader = Reader::new(bytes);
        let tag = reader.u8()?;
        let body = reader.remaining().to_vec();
        match tag {
            TAG_OK => Ok(Reply::Ok(body)),
            TAG_FAULT => Ok(Reply::Fault(body)),
            _ => Err(Error("Reply tag must be 0 (ok) or 1 (fault)")),
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut writer = Writer::new();
        let (tag, body) = match self {
            Reply::Ok(body) => (TAG_OK, body),
            Reply::Fault(body) => (TAG_FAULT, body),
        };
        writer.u8(tag).remainder(body);
        writer.into_bytes()
    }
}

/// The answer to one Yield Call. Carries a `Break` outcome the dispatch
/// Reply has no counterpart for, because a block can end its caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum YieldReply {
    /// The block completed; the bytes are its value, codec-encoded.
    Ok(Vec<u8>),
    /// The block ran `break`; the bytes are the break value,
    /// codec-encoded. It returns to the guest rather than to host code.
    Break(Vec<u8>),
    /// The block raised, or failed in a way the host re-raises at the
    /// yield site.
    Error(ErrorRecord),
}

impl YieldReply {
    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        let mut reader = Reader::new(bytes);
        let tag = reader.u8()?;
        match tag {
            YIELD_OK => Ok(YieldReply::Ok(reader.remaining().to_vec())),
            YIELD_BREAK => Ok(YieldReply::Break(reader.remaining().to_vec())),
            YIELD_ERROR => {
                let record = ErrorRecord::read(&mut reader)?;
                reader.finish()?;
                Ok(YieldReply::Error(record))
            }
            YIELD_RESERVED => Err(Error("Yield Reply tag 0x03 is reserved")),
            _ => Err(Error("Yield Reply tag is not recognised")),
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut writer = Writer::new();
        match self {
            YieldReply::Ok(body) => {
                writer.u8(YIELD_OK).remainder(body);
            }
            YieldReply::Break(body) => {
                writer.u8(YIELD_BREAK).remainder(body);
            }
            YieldReply::Error(record) => {
                writer.u8(YIELD_ERROR);
                record.write(&mut writer);
            }
        }
        writer.into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_reply_arms_round_trip() {
        for reply in [Reply::Ok(vec![0x2a]), Reply::Fault(vec![0xc7, 0x00, 0x02])] {
            let encoded = reply.encode();
            assert_eq!(
                Reply::decode(&encoded),
                Ok(reply),
                "a Reply must survive a host encode and decode on both arms"
            );
        }
    }

    #[test]
    fn the_fault_arm_is_distinguishable_without_reading_the_body() {
        // Identical bodies, different arms: the tag alone must separate
        // "the Service returned this" from "the Service failed with this".
        let body = vec![0x01, 0x02];
        assert_ne!(
            Reply::Ok(body.clone()).encode(),
            Reply::Fault(body).encode(),
            "ok and fault must differ in the envelope, not only in the payload"
        );
    }

    #[test]
    fn an_empty_ok_body_round_trips() {
        let encoded = Reply::Ok(Vec::new()).encode();
        assert_eq!(
            Reply::decode(&encoded),
            Ok(Reply::Ok(Vec::new())),
            "a Reply whose payload is empty must decode as an empty value, not a truncation"
        );
    }

    #[test]
    fn an_unknown_reply_tag_is_refused() {
        assert!(
            Reply::decode(&[9, 0]).is_err(),
            "a Reply tag that is neither ok nor fault must be rejected as a wire violation"
        );
    }

    #[test]
    fn a_zero_length_reply_is_refused() {
        assert!(
            Reply::decode(&[]).is_err(),
            "a zero-length Reply must be rejected rather than read as a missing tag"
        );
    }

    #[test]
    fn every_live_yield_arm_round_trips() {
        let arms = [
            YieldReply::Ok(vec![0xa4]),
            YieldReply::Break(vec![0xa4]),
            YieldReply::Error(ErrorRecord {
                class: "LocalJumpError".into(),
                message: "no block".into(),
                backtrace: vec!["(eval):1".into()],
            }),
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

    #[test]
    fn the_reserved_yield_tag_is_refused() {
        assert!(
            YieldReply::decode(&[YIELD_RESERVED]).is_err(),
            "Yield Reply tag 0x03 must be rejected by both peers so it stays reserved"
        );
    }

    #[test]
    fn an_unknown_yield_tag_is_refused() {
        assert!(
            YieldReply::decode(&[0x7f]).is_err(),
            "a Yield Reply tag outside the live set must be rejected as a wire violation"
        );
    }

    #[test]
    fn golden_layout_pins_the_yield_tags() {
        assert_eq!(
            YieldReply::Break(vec![0xc0]).encode(),
            vec![0x02, 0xc0],
            "the Yield Reply break tag and body layout must stay fixed for both peers"
        );
    }
}
