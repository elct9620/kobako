//! The guest's report that something it was running raised — guest side.
//!
//! A block failure and an invocation failure carry the same three fields,
//! so the host re-raises from either without consulting a payload codec.

use super::bytes::{put_bytes, put_list, take_text, take_text_list};
use super::Error;

/// Exception class, message, and backtrace as the guest saw them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ErrorRecord {
    pub class: String,
    pub message: String,
    pub backtrace: Vec<String>,
}

impl ErrorRecord {
    pub fn take(bytes: &[u8], at: &mut usize) -> Result<Self, Error> {
        let class = take_text(bytes, at)?.to_owned();
        let message = take_text(bytes, at)?.to_owned();
        Ok(ErrorRecord {
            class,
            message,
            backtrace: take_text_list(bytes, at)?,
        })
    }

    pub fn put(&self, out: &mut Vec<u8>) {
        put_bytes(out, self.class.as_bytes());
        put_bytes(out, self.message.as_bytes());
        put_list(out, &self.backtrace);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_the_wire() {
        let record = ErrorRecord {
            class: "RuntimeError".into(),
            message: "boom".into(),
            backtrace: vec!["(eval):1".into()],
        };
        let mut out = Vec::new();
        record.put(&mut out);
        let mut at = 0;
        assert_eq!(
            ErrorRecord::take(&out, &mut at),
            Ok(record),
            "an Error Record must survive a guest encode and decode unchanged"
        );
    }

    #[test]
    fn golden_layout_is_class_then_message_then_backtrace() {
        let record = ErrorRecord {
            class: "E".into(),
            message: "m".into(),
            backtrace: vec!["b".into()],
        };
        let mut out = Vec::new();
        record.put(&mut out);
        assert_eq!(
            out,
            vec![
                0, 0, 0, 1, b'E', //
                0, 0, 0, 1, b'm', //
                0, 0, 0, 1, //
                0, 0, 0, 1, b'b',
            ],
            "the Error Record byte layout must stay fixed for both peers to agree"
        );
    }

    #[test]
    fn an_empty_backtrace_is_legal() {
        let record = ErrorRecord {
            class: "E".into(),
            message: "m".into(),
            backtrace: Vec::new(),
        };
        let mut out = Vec::new();
        record.put(&mut out);
        let mut at = 0;
        assert_eq!(
            ErrorRecord::take(&out, &mut at),
            Ok(record),
            "a guest failure with no backtrace must round-trip as an empty list"
        );
    }
}
