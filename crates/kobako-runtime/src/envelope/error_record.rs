//! The guest's report that something it was running raised.
//!
//! A block failure and an invocation failure carry the same three fields,
//! so the host re-raises from either without consulting a payload codec.
//! Distinct from a Fault, which travels the other way and is categorized
//! by one of three reserved type names the guest maps to a proxy-side
//! error.

use super::bytes::{Reader, Writer};
use super::Error;

/// Exception class, message, and backtrace as the guest saw them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ErrorRecord {
    pub class: String,
    pub message: String,
    pub backtrace: Vec<String>,
}

impl ErrorRecord {
    pub fn read(reader: &mut Reader<'_>) -> Result<Self, Error> {
        Ok(ErrorRecord {
            class: reader.text()?.to_owned(),
            message: reader.text()?.to_owned(),
            backtrace: reader.text_list()?,
        })
    }

    pub fn write(&self, writer: &mut Writer) {
        writer
            .bytes(self.class.as_bytes())
            .bytes(self.message.as_bytes())
            .list(&self.backtrace);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ErrorRecord {
        ErrorRecord {
            class: "RuntimeError".into(),
            message: "boom".into(),
            backtrace: vec!["(eval):1".into()],
        }
    }

    #[test]
    fn round_trips_through_the_wire() {
        let mut w = Writer::new();
        sample().write(&mut w);
        let encoded = w.into_bytes();
        let mut r = Reader::new(&encoded);
        assert_eq!(
            ErrorRecord::read(&mut r),
            Ok(sample()),
            "an Error Record must survive a host encode and decode unchanged"
        );
    }

    #[test]
    fn an_empty_backtrace_is_legal() {
        let record = ErrorRecord {
            backtrace: Vec::new(),
            ..sample()
        };
        let mut w = Writer::new();
        record.write(&mut w);
        let encoded = w.into_bytes();
        let mut r = Reader::new(&encoded);
        assert_eq!(
            ErrorRecord::read(&mut r),
            Ok(record),
            "a guest failure with no backtrace must round-trip as an empty list, not a decode error"
        );
    }

    #[test]
    fn golden_layout_is_class_then_message_then_backtrace() {
        let record = ErrorRecord {
            class: "E".into(),
            message: "m".into(),
            backtrace: vec!["b".into()],
        };
        let mut w = Writer::new();
        record.write(&mut w);
        assert_eq!(
            w.into_bytes(),
            vec![
                0, 0, 0, 1, b'E', // class
                0, 0, 0, 1, b'm', // message
                0, 0, 0, 1, // backtrace count
                0, 0, 0, 1, b'b',
            ],
            "the Error Record byte layout must stay fixed for both peers to agree"
        );
    }
}
