//! The Reply envelope and its reverse-direction sibling, the Yield Reply —
//! guest side.
//!
//! The guest reads a Reply after every dispatch it initiates and writes a
//! Yield Reply after every block the host re-enters, so both directions
//! meet here.

use super::bytes::{expect_end, rest, take_u8};
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
    /// The method returned; the bytes are the value, adapter-encoded.
    Ok(Vec<u8>),
    /// The method refused or failed; the bytes are the fault,
    /// adapter-encoded.
    Fault(Vec<u8>),
}

impl Reply {
    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        let mut at = 0usize;
        let tag = take_u8(bytes, &mut at)?;
        let body = rest(bytes, &at).to_vec();
        match tag {
            TAG_OK => Ok(Reply::Ok(body)),
            TAG_FAULT => Ok(Reply::Fault(body)),
            _ => Err(Error("Reply tag must be 0 (ok) or 1 (fault)")),
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let (tag, body) = match self {
            Reply::Ok(body) => (TAG_OK, body),
            Reply::Fault(body) => (TAG_FAULT, body),
        };
        let mut out = Vec::with_capacity(1 + body.len());
        out.push(tag);
        out.extend_from_slice(body);
        out
    }
}

/// The answer to one Yield Call. Carries a `Break` outcome the dispatch
/// Reply has no counterpart for, because a block can end its caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum YieldReply {
    /// The block completed; the bytes are its value, adapter-encoded.
    Ok(Vec<u8>),
    /// The block ran `break`; the bytes are the break value,
    /// adapter-encoded. It returns to the guest rather than to host code.
    Break(Vec<u8>),
    /// The block raised, or failed in a way the host re-raises at the
    /// yield site.
    Error(ErrorRecord),
}

impl YieldReply {
    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        let mut at = 0usize;
        match take_u8(bytes, &mut at)? {
            YIELD_OK => Ok(YieldReply::Ok(rest(bytes, &at).to_vec())),
            YIELD_BREAK => Ok(YieldReply::Break(rest(bytes, &at).to_vec())),
            YIELD_ERROR => {
                let record = ErrorRecord::take(bytes, &mut at)?;
                expect_end(bytes, &at)?;
                Ok(YieldReply::Error(record))
            }
            YIELD_RESERVED => Err(Error("Yield Reply tag 0x03 is reserved")),
            _ => Err(Error("Yield Reply tag is not recognised")),
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        match self {
            YieldReply::Ok(body) => {
                out.push(YIELD_OK);
                out.extend_from_slice(body);
            }
            YieldReply::Break(body) => {
                out.push(YIELD_BREAK);
                out.extend_from_slice(body);
            }
            YieldReply::Error(record) => {
                out.push(YIELD_ERROR);
                record.put(&mut out);
            }
        }
        out
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
                "a Reply must survive a guest encode and decode on both arms"
            );
        }
    }

    #[test]
    fn the_fault_arm_is_distinguishable_without_reading_the_body() {
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
    fn a_zero_length_reply_is_refused() {
        assert!(
            Reply::decode(&[]).is_err(),
            "a zero-length Reply must be rejected rather than read as a missing tag"
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
                "every live Yield Reply arm must survive a guest encode and decode"
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
    fn golden_layout_pins_the_yield_tags() {
        assert_eq!(
            YieldReply::Break(vec![0xc0]).encode(),
            vec![0x02, 0xc0],
            "the Yield Reply break tag and body layout must stay fixed for both peers"
        );
    }
}
