//! A receiver saying it does not serve the operation it was sent.
//!
//! It is an answer rather than a breakage, so it rides every operation's
//! answer as its own arm and the caller decides whether to fall back or
//! give up. It names no operation: the caller knows what it sent.

use super::bytes::{Reader, Writer};
use super::{tag, DecodeError};

/// The only kind defined. A kind this reader predates reads as this one,
/// so a later kind still delivers its message.
const KIND_UNSUPPORTED: u8 = 0;

/// The refusal a receiver answers with when it does not serve an
/// operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unsupported {
    pub message: String,
}

impl Unsupported {
    pub fn new(message: impl Into<String>) -> Self {
        Unsupported {
            message: message.into(),
        }
    }

    /// This refusal as a whole answer. Every operation's answer spells
    /// the arm the same way, so a receiver refusing a tag it cannot
    /// read needs no answer type to refuse it in.
    pub fn encode(&self) -> Vec<u8> {
        let mut writer = Writer::new();
        writer
            .u8(tag::UNSUPPORTED)
            .u8(KIND_UNSUPPORTED)
            .bytes(self.message.as_bytes());
        writer.into_bytes()
    }

    pub(crate) fn read(reader: &mut Reader<'_>) -> Result<Self, DecodeError> {
        reader.u8()?;
        let message = reader.text()?.to_owned();
        // The last thing its answer carries, so whatever follows belongs
        // to a later version of these fields, as for a Fault.
        reader.skip_unknown_fields();
        Ok(Unsupported { message })
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Outcome, Reply, YieldReply};
    use super::*;

    // @behavior WE-069
    #[test]
    fn an_unsupported_answer_round_trips_on_every_operation() {
        let refusal = Unsupported::new("not served");
        assert_eq!(
            refusal.encode(),
            Reply::Unsupported(refusal.clone()).encode(),
            "an Unsupported encoded on its own must be the same answer every operation carries"
        );
        assert_eq!(
            (
                Reply::decode(&Reply::Unsupported(refusal.clone()).encode()),
                YieldReply::decode(&YieldReply::Unsupported(refusal.clone()).encode()),
                Outcome::decode(&Outcome::Unsupported(refusal.clone()).encode()),
            ),
            (
                Ok(Reply::Unsupported(refusal.clone())),
                Ok(YieldReply::Unsupported(refusal.clone())),
                Ok(Outcome::Unsupported(refusal)),
            ),
            "an Unsupported through each answer's encode and decode must arrive as it was \
             given"
        );
    }

    // @behavior WE-070
    #[test]
    fn a_kind_this_reader_predates_still_delivers_its_message() {
        let bytes = [0x15, 9, 0, 0, 0, 1, b'm', 0xff];
        assert_eq!(
            Reply::decode(&bytes),
            Ok(Reply::Unsupported(Unsupported::new("m"))),
            "an Unsupported of an unknown kind with a trailing field through Reply::decode \
             must read as the one kind defined, its message intact"
        );
    }
}
