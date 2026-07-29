//! The guest-supplied block as a receiver observes it.
//!
//! When a guest call site supplies a block, the dispatch frame hands
//! the receiver a `Yielder` in the `block` parameter; each `call` is a
//! synchronous yield round-trip into the in-flight guest. A `Yielder`
//! borrows its dispatch frame, so it cannot outlive the dispatch —
//! where the Ruby frontend refuses an escaped Yielder at runtime, this
//! API makes the escape a compile error.

use std::fmt;

use kobako_runtime::yielder::Yielder as RawYielder;
use kobako_transport::envelope::YieldReply;

use crate::receiver::{Fault, FaultKind};

/// A yield round-trip that did not come back with a plain value.
///
/// `From<YieldError> for Fault` lets a receiver propagate with `?`; the
/// dispatch frame gives each variant its contractual meaning, so a
/// receiver only ever needs to stop and hand the error up.
///
/// Non-exhaustive because a receiver matches it to recover, and the
/// yield-outcome kinds grow append-only; keep a wildcard arm.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum YieldError {
    /// The guest block terminated the call with `break`: the receiver
    /// must stop; the dispatch answers the guest with the break value
    /// no matter what the receiver returns after this.
    Break,
    /// The block body raised, or its value could not ride the wire.
    /// The receiver observes it at the yield site and may recover or
    /// propagate.
    Failure { name: String, message: String },
    /// The re-entry itself failed — the guest trapped mid-block or
    /// answered with malformed Yield Reply bytes.
    Aborted(String),
}

impl fmt::Display for YieldError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            YieldError::Break => f.write_str("guest block break crossed the receiver"),
            YieldError::Failure { name, message } => write!(f, "{name}: {message}"),
            YieldError::Aborted(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for YieldError {}

impl From<YieldError> for Fault {
    /// Every variant folds to a `runtime` fault: a propagated block
    /// failure is a Service-layer failure to the guest, and a
    /// propagated `Break` never reaches the guest at all — the
    /// dispatch answers with the break value first.
    fn from(err: YieldError) -> Self {
        Fault::new(FaultKind::Runtime, err.to_string())
    }
}

/// Host-side stand-in for the guest block of one dispatch frame.
pub struct Yielder<'y> {
    channel: &'y mut dyn RawYielder,
    broke: Option<Vec<u8>>,
}

impl<'y> Yielder<'y> {
    pub(crate) fn new(channel: &'y mut dyn RawYielder) -> Self {
        Yielder {
            channel,
            broke: None,
        }
    }

    /// Run the guest block once with `args` and return its value.
    ///
    /// The value arrives as the raw wire `Value`: a `Value::Handle`
    /// inside it stays a token until the receiver resolves it through
    /// `Handles` — the explicit spelling of the Ruby frontend's
    /// automatic restore at the yield site.
    ///
    /// A `break` in the block ends the receiver call: this returns
    /// `YieldError::Break` now and on every later call, without
    /// re-entering the guest.
    pub fn call_payload(&mut self, args: &[u8]) -> Result<Vec<u8>, YieldError> {
        if self.broke.is_some() {
            return Err(YieldError::Break);
        }
        let bytes = self
            .channel
            .yield_to_block(args)
            .map_err(|trap| YieldError::Aborted(format!("yield re-entry trapped: {trap:?}")))?;
        let reply = YieldReply::decode(&bytes)
            .map_err(|err| YieldError::Aborted(format!("malformed Yield Reply: {err}")))?;
        match reply {
            YieldReply::Ok(body) => Ok(body),
            YieldReply::Break(body) => {
                self.broke = Some(body);
                Err(YieldError::Break)
            }
            YieldReply::Error(record) => Err(YieldError::Failure {
                name: record.name,
                message: record.message,
            }),
        }
    }

    /// The recorded break value as the guest wrote it, consumed by the
    /// dispatch frame once the receiver returns. Bytes, so the frame
    /// answers with what the block produced rather than a re-encoding.
    pub(crate) fn into_break(self) -> Option<Vec<u8>> {
        self.broke
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use kobako_runtime::error::Trap;
    use kobako_transport::envelope::ErrorRecord;

    use super::*;

    /// A raw yield channel answering from a canned script, recording
    /// what the Yielder sent into the guest.
    struct Scripted {
        responses: VecDeque<Result<Vec<u8>, Trap>>,
        sent: Vec<Vec<u8>>,
    }

    impl Scripted {
        fn new(responses: Vec<Result<Vec<u8>, Trap>>) -> Self {
            Scripted {
                responses: responses.into(),
                sent: Vec::new(),
            }
        }
    }

    impl RawYielder for Scripted {
        fn yield_to_block(&mut self, args: &[u8]) -> Result<Vec<u8>, Trap> {
            self.sent.push(args.to_vec());
            self.responses.pop_front().expect("script exhausted")
        }
    }

    #[test]
    fn call_payload_ships_the_bytes_it_was_handed_and_returns_the_ok_body() {
        let mut channel = Scripted::new(vec![Ok(YieldReply::Ok(vec![0x2a]).encode())]);
        let mut block = Yielder::new(&mut channel);

        let body = block.call_payload(&[0x15]).unwrap();

        assert_eq!(
            (body, channel.sent),
            (vec![0x2a], vec![vec![0x15]]),
            "a yield through call_payload must ride the host's own bytes in both \
             directions, since the schema is the host's"
        );
    }

    #[test]
    fn break_records_the_body_and_stops_re_entering_the_guest() {
        let mut channel = Scripted::new(vec![Ok(YieldReply::Break(vec![0x2a]).encode())]);
        let mut block = Yielder::new(&mut channel);

        assert_eq!(block.call_payload(&[]), Err(YieldError::Break));
        assert_eq!(block.call_payload(&[]), Err(YieldError::Break));
        assert_eq!(
            (block.into_break(), channel.sent.len()),
            (Some(vec![0x2a]), 1),
            "a block that broke must hand its body back as the guest wrote it, and a \
             broken Yielder must not re-enter the guest again"
        );
    }

    #[test]
    fn the_error_arm_surfaces_the_records_name_and_message() {
        let record = YieldReply::Error(ErrorRecord {
            name: "LocalJumpError".into(),
            message: "boom".into(),
            backtrace: vec!["(eval):1".into()],
        });
        let mut channel = Scripted::new(vec![Ok(record.encode())]);
        let mut block = Yielder::new(&mut channel);

        assert_eq!(
            block.call_payload(&[]),
            Err(YieldError::Failure {
                name: "LocalJumpError".into(),
                message: "boom".into(),
            }),
            "a block that raised must reach the receiver as its own error name and message"
        );
    }

    #[test]
    fn trap_during_re_entry_aborts() {
        let mut channel = Scripted::new(vec![Err(Trap::Timeout("deadline".into()))]);
        let mut block = Yielder::new(&mut channel);

        assert!(
            matches!(block.call_payload(&[]), Err(YieldError::Aborted(_))),
            "a guest that trapped mid-block must abort the yield rather than answer it"
        );
    }

    #[test]
    fn malformed_response_bytes_abort() {
        let mut channel = Scripted::new(vec![Ok(vec![0x03, 0xc0])]);
        let mut block = Yielder::new(&mut channel);

        assert!(
            matches!(block.call_payload(&[]), Err(YieldError::Aborted(_))),
            "bytes the envelope cannot frame as a Yield Reply must abort the yield"
        );
    }

    #[test]
    fn every_yield_error_folds_to_a_runtime_fault() {
        let failure = YieldError::Failure {
            name: "LocalJumpError".into(),
            message: "crossed".into(),
        };
        let fault = Fault::from(failure);
        assert_eq!(fault.kind, FaultKind::Runtime);
        assert_eq!(fault.message, "LocalJumpError: crossed");
        assert_eq!(Fault::from(YieldError::Break).kind, FaultKind::Runtime);
        assert_eq!(
            Fault::from(YieldError::Aborted("gone".into())).kind,
            FaultKind::Runtime
        );
    }
}
