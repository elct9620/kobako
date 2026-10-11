//! The answer to a compile downcall.
//!
//! Only the guest can compile its own source, so the host learns the
//! bytecode, or why there is none, from this answer alone.

use super::bytes::{Reader, Writer};
use super::{tag, DecodeError, ErrorRecord, Unsupported};

/// How compiling the Preamble's source Snippets ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompileReply {
    /// One bytecode per source Snippet, in the Preamble's order.
    Ok(Vec<Vec<u8>>),
    /// A Snippet did not compile, or a Flag could not be applied first.
    Error(ErrorRecord),
    /// The guest does not compile.
    Unsupported(Unsupported),
}

impl CompileReply {
    pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(bytes);
        match reader.u8()? {
            tag::OK => {
                let bytecodes = reader.bytes_list()?;
                reader.finish()?;
                Ok(CompileReply::Ok(bytecodes))
            }
            tag::ERROR => {
                let record = ErrorRecord::read(&mut reader)?;
                reader.finish()?;
                Ok(CompileReply::Error(record))
            }
            tag::UNSUPPORTED => Ok(CompileReply::Unsupported(Unsupported::read(&mut reader)?)),
            _ => Err(DecodeError::new(
                "Compile Reply tag must be 0x10 (ok), 0x13 (error), or 0x15 (unsupported)",
            )),
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut writer = Writer::new();
        match self {
            CompileReply::Ok(bytecodes) => {
                writer.u8(tag::OK).list(bytecodes);
            }
            CompileReply::Error(record) => {
                writer.u8(tag::ERROR);
                record.write(&mut writer);
            }
            CompileReply::Unsupported(refusal) => return refusal.encode(),
        }
        writer.into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // @behavior WE-085
    #[test]
    fn every_compile_reply_arm_round_trips() {
        for reply in [
            CompileReply::Ok(vec![vec![0x52, 0x49], Vec::new()]),
            CompileReply::Ok(Vec::new()),
            CompileReply::Error(ErrorRecord {
                name: "SyntaxError".into(),
                message: "unexpected end".into(),
                backtrace: vec!["(snippet:A):1".into()],
            }),
            CompileReply::Unsupported(Unsupported::new("not compiled here")),
        ] {
            assert_eq!(
                CompileReply::decode(&reply.encode()),
                Ok(reply),
                "a Compile Reply through encode and decode must arrive as it was given on \
                 every arm"
            );
        }
    }

    // @behavior WE-086
    #[test]
    fn golden_layout_pins_the_compile_reply_tags() {
        assert_eq!(
            CompileReply::Ok(vec![vec![0xaa], vec![0xbb, 0xcc]]).encode(),
            vec![
                0x10, // tag: ok
                0, 0, 0, 2, // bytecode count
                0, 0, 0, 1, 0xaa, // first bytecode
                0, 0, 0, 2, 0xbb, 0xcc, // second bytecode
            ],
            "a compiled answer must encode as tag byte 0x10 followed by a counted list of \
             length-prefixed bytecodes"
        );
        assert_eq!(
            CompileReply::Error(ErrorRecord {
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
            "a failed compile must encode as tag byte 0x13 followed by an Error Record"
        );
        assert_eq!(
            CompileReply::Unsupported(Unsupported::new("x")).encode(),
            vec![0x15, 0, 0, 0, 0, 1, b'x'],
            "a guest that does not compile must answer on tag byte 0x15"
        );
    }

    // @behavior WE-087
    #[test]
    fn a_compile_reply_tag_outside_its_arms_is_refused() {
        for bytes in [&[0x11, 0, 0, 0, 0][..], &[0x14][..], &[][..]] {
            assert!(
                CompileReply::decode(bytes).is_err(),
                "a Compile Reply {bytes:02x?} through CompileReply::decode must be refused"
            );
        }
    }

    // @behavior WE-088
    #[test]
    fn a_compiled_answer_with_trailing_bytes_is_refused() {
        assert!(
            CompileReply::decode(&[0x10, 0, 0, 0, 0, 0xff]).is_err(),
            "a compiled answer with bytes after its list through CompileReply::decode must be \
             refused as a framing desync"
        );
    }
}
