//! The guest's report that something it was running raised.
//!
//! A block failure and an invocation failure carry the same three fields,
//! so the host re-raises from either without consulting a payload codec.
//! Distinct from a Fault, which travels the other way and is categorized
//! by a reserved category name the guest maps to a proxy-side error.

use super::bytes::{Reader, Writer};
use super::DecodeError;

/// The name a wire violation carries: an exchange that did not complete.
pub const TRANSPORT_ERROR: &str = "Kobako::Transport::Error";
/// The name a guest that could not start the Sandbox carries.
pub const BOOT_ERROR: &str = "Kobako::BootError";
/// The name a failure the guest's own flow detected carries, where no script
/// error names one.
pub const SANDBOX_ERROR: &str = "Kobako::SandboxError";
/// The name a Snippet the guest could not load carries.
pub const BYTECODE_ERROR: &str = "Kobako::BytecodeError";
/// The name a critical Preamble flag the guest does not know carries.
pub const UNSUPPORTED_FLAG_ERROR: &str = "Kobako::UnsupportedFlagError";
/// The name a Run whose entrypoint the guest cannot resolve carries.
pub const UNDEFINED_ENTRYPOINT_ERROR: &str = "Kobako::UndefinedEntrypointError";
/// The name a Service call that failed carries, where no Fault category
/// narrows it.
pub const SERVICE_ERROR: &str = "Kobako::ServiceError";
/// The name a call to a Service the host does not hold carries.
pub const NO_SERVICE_ERROR: &str = "Kobako::NoServiceError";
/// The name a Service call the host refused for its arguments carries.
pub const SERVICE_ARGUMENT_ERROR: &str = "Kobako::ServiceArgumentError";

/// The error's name, message, and backtrace as the guest saw them. `name`
/// rather than `class` because a guest need not be object-oriented to have
/// named the error it is reporting.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ErrorRecord {
    pub name: String,
    pub message: String,
    pub backtrace: Vec<String>,
}

impl ErrorRecord {
    pub(crate) fn read(reader: &mut Reader<'_>) -> Result<Self, DecodeError> {
        Ok(ErrorRecord {
            name: reader.read_text()?.to_owned(),
            message: reader.read_text()?.to_owned(),
            backtrace: reader.read_text_list()?,
        })
    }

    pub(crate) fn write(&self, writer: &mut Writer) {
        writer
            .bytes(self.name.as_bytes())
            .bytes(self.message.as_bytes())
            .list(&self.backtrace);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ErrorRecord {
        ErrorRecord {
            name: "RuntimeError".into(),
            message: "boom".into(),
            backtrace: vec!["(eval):1".into()],
        }
    }

    // @behavior WE-047
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

    // @behavior WE-048
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

    // @behavior WE-049
    #[test]
    fn golden_layout_is_name_then_message_then_backtrace() {
        let record = ErrorRecord {
            name: "E".into(),
            message: "m".into(),
            backtrace: vec!["b".into()],
        };
        let mut w = Writer::new();
        record.write(&mut w);
        assert_eq!(
            w.into_bytes(),
            vec![
                0, 0, 0, 1, b'E', // name
                0, 0, 0, 1, b'm', // message
                0, 0, 0, 1, // backtrace count
                0, 0, 0, 1, b'b',
            ],
            "the Error Record byte layout must stay fixed for both peers to agree"
        );
    }
}
