//! The host refusing or failing a Call.
//!
//! Every byte of a Fault is kobako's — a closed category and a message —
//! so it rides the envelope and a guest reads a refusal with no payload
//! codec at all.
//!
//! Distinct from an `ErrorRecord`, which travels the other way. The
//! direction decides what each carries: a Fault crosses host to guest,
//! where a backtrace would put host paths and object graphs in front of
//! untrusted code, so it structurally has none.

use super::bytes::{Reader, Writer};
use super::DecodeError;

const KIND_RUNTIME: u8 = 0;
const KIND_ARGUMENT: u8 = 1;
const KIND_UNDEFINED: u8 = 2;

/// Which failure a Fault reports.
///
/// The set grows, so a reader that meets a category it does not know
/// degrades rather than failing the exchange. `Undefined` stays
/// indistinguishable across its causes — an unbound path, an unknown
/// method, a rejected name — so a guest probing the surface learns
/// nothing from which refusal it got, which is also what makes it the
/// landing place for a category this reader predates: it attributes
/// nothing to the Service that a newer category might contradict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum FaultKind {
    /// A Ruby exception raised inside the Service method.
    Runtime,
    /// The call shape did not fit the method.
    Argument,
    /// No such member or method.
    Undefined,
}

impl FaultKind {
    fn tag(self) -> u8 {
        match self {
            FaultKind::Runtime => KIND_RUNTIME,
            FaultKind::Argument => KIND_ARGUMENT,
            FaultKind::Undefined => KIND_UNDEFINED,
        }
    }

    /// Read a category from its tag, degrading one this reader predates
    /// to the category that claims the least about what failed.
    fn from_tag(tag: u8) -> Self {
        match tag {
            KIND_RUNTIME => FaultKind::Runtime,
            KIND_ARGUMENT => FaultKind::Argument,
            _ => FaultKind::Undefined,
        }
    }

    /// The spelling the guest raises under and a frontend reports.
    pub fn name(self) -> &'static str {
        match self {
            FaultKind::Runtime => "runtime",
            FaultKind::Argument => "argument",
            FaultKind::Undefined => "undefined",
        }
    }

    /// Read a category from the name a frontend uses, or `None` when the
    /// name is outside the closed set.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "runtime" => Some(FaultKind::Runtime),
            "argument" => Some(FaultKind::Argument),
            "undefined" => Some(FaultKind::Undefined),
            _ => None,
        }
    }
}

/// A Service-level refusal: the guest re-raises it as a rescuable
/// exception, never a wasm trap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fault {
    pub kind: FaultKind,
    pub message: String,
}

impl Fault {
    pub fn new(kind: FaultKind, message: impl Into<String>) -> Self {
        Fault {
            kind,
            message: message.into(),
        }
    }

    pub(crate) fn read(reader: &mut Reader<'_>) -> Result<Self, DecodeError> {
        let kind = FaultKind::from_tag(reader.u8()?);
        let message = reader.text()?.to_owned();
        // A Fault gains fields as the contract does, and it is the last
        // thing a Reply carries, so whatever follows the fields this
        // reader knows belongs to a later version of them.
        reader.skip_unknown_fields();
        Ok(Fault { kind, message })
    }

    pub(crate) fn write(&self, writer: &mut Writer) {
        writer.u8(self.kind.tag()).bytes(self.message.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_round_trips_through_the_wire() {
        for kind in [
            FaultKind::Runtime,
            FaultKind::Argument,
            FaultKind::Undefined,
        ] {
            let fault = Fault::new(kind, "boom");
            let mut w = Writer::new();
            fault.write(&mut w);
            let encoded = w.into_bytes();
            let mut r = Reader::new(&encoded);
            assert_eq!(
                Fault::read(&mut r),
                Ok(fault),
                "every Fault kind must survive an encode and decode unchanged"
            );
        }
    }

    #[test]
    fn a_kind_the_reader_does_not_know_degrades_to_undefined() {
        let encoded = vec![0xfe, 0, 0, 0, 1, b'x'];
        let mut r = Reader::new(&encoded);
        assert_eq!(
            Fault::read(&mut r),
            Ok(Fault::new(FaultKind::Undefined, "x")),
            "a Fault category added after this reader was built must deliver its message \
             under the category that claims the least, not fail the exchange"
        );
    }

    #[test]
    fn a_field_the_reader_does_not_know_is_skipped() {
        let mut w = Writer::new();
        Fault::new(FaultKind::Undefined, "no such method").write(&mut w);
        w.bytes(b"a field a later contract version appended");
        let encoded = w.into_bytes();
        let mut r = Reader::new(&encoded);
        assert_eq!(
            Fault::read(&mut r),
            Ok(Fault::new(FaultKind::Undefined, "no such method")),
            "a Fault field added after this reader was built must be skipped, so the \
             fields it does know still arrive"
        );
    }

    #[test]
    fn golden_layout_is_kind_then_message() {
        let mut w = Writer::new();
        Fault::new(FaultKind::Argument, "m").write(&mut w);
        assert_eq!(
            w.into_bytes(),
            vec![
                1, // kind: argument
                0, 0, 0, 1, b'm', // message
            ],
            "the Fault byte layout must stay fixed for both peers to agree"
        );
    }

    #[test]
    fn names_map_both_ways() {
        for kind in [
            FaultKind::Runtime,
            FaultKind::Argument,
            FaultKind::Undefined,
        ] {
            assert_eq!(
                FaultKind::from_name(kind.name()),
                Some(kind),
                "a kind's name must read back as the same kind"
            );
        }
        assert_eq!(
            FaultKind::from_name("other"),
            None,
            "a name outside the closed set must not resolve to a kind"
        );
    }
}
