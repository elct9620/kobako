//! The guest-supplied block as a receiver observes it.
//!
//! When a guest call site supplies a block, the dispatch frame hands
//! the receiver a `Yielder` in the `block` parameter; each `call` is a
//! synchronous yield round-trip into the in-flight guest. A `Yielder`
//! borrows its dispatch frame, so it cannot outlive the dispatch —
//! where the Ruby frontend refuses an escaped Yielder at runtime, this
//! API makes the escape a compile error.

use std::fmt;

use kobako_codec::codec::{Decode as _, Encoder, Value};
use kobako_codec::transport::{Yield, TAG_BREAK, TAG_OK};
use kobako_runtime::yielder::Yielder as RawYielder;

use crate::receiver::{Fault, FaultKind};

/// A yield round-trip that did not come back with a plain value.
///
/// `From<YieldError> for Fault` lets a member propagate with `?`; the
/// dispatch frame gives each variant its contractual meaning, so a
/// member only ever needs to stop and hand the error up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum YieldError {
    /// The guest block terminated the call with `break`: the member
    /// must stop; the dispatch answers the guest with the break value
    /// no matter what the member returns after this.
    Break,
    /// The block body raised, or its value could not ride the wire.
    /// The member observes it at the yield site and may recover or
    /// propagate.
    Failure { class: String, message: String },
    /// The re-entry itself failed — the guest trapped mid-block or
    /// answered with malformed YieldResponse bytes.
    Aborted(String),
}

impl fmt::Display for YieldError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            YieldError::Break => f.write_str("guest block break crossed the member"),
            YieldError::Failure { class, message } => write!(f, "{class}: {message}"),
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
    broke: Option<Value>,
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
    /// inside it stays a token until the member resolves it through
    /// `Handles` — the explicit spelling of the Ruby frontend's
    /// automatic restore at the yield site.
    ///
    /// A `break` in the block ends the member call: this returns
    /// `YieldError::Break` now and on every later call, without
    /// re-entering the guest.
    pub fn call(&mut self, args: &[Value]) -> Result<Value, YieldError> {
        if self.broke.is_some() {
            return Err(YieldError::Break);
        }
        let payload = encode_args(args)?;
        let bytes = self
            .channel
            .yield_block(&payload)
            .map_err(|trap| YieldError::Aborted(format!("yield re-entry trapped: {trap:?}")))?;
        let response = Yield::decode(&bytes)
            .map_err(|err| YieldError::Aborted(format!("malformed YieldResponse: {err}")))?;
        match response.tag {
            TAG_OK => Ok(response.value),
            TAG_BREAK => {
                self.broke = Some(response.value);
                Err(YieldError::Break)
            }
            // `Yield::decode` admits only live tags; the remainder is
            // the error tag.
            _ => Err(failure(response.value)),
        }
    }

    /// The recorded break value, consumed by the dispatch frame once
    /// the member returns.
    pub(crate) fn into_break(self) -> Option<Value> {
        self.broke
    }
}

/// Positional yield arguments ride as one msgpack array, the same
/// shape the Ruby Yielder encodes.
fn encode_args(args: &[Value]) -> Result<Vec<u8>, YieldError> {
    let mut encoder = Encoder::new();
    encoder
        .write_value(&Value::Array(args.to_vec()))
        .map_err(|err| YieldError::Aborted(format!("yield arguments are not encodable: {err}")))?;
    Ok(encoder.into_bytes())
}

/// Reify a tag `0x04` payload — a `{"class", "message", "backtrace"}`
/// map — with the same fallbacks the Ruby Yielder applies to a
/// malformed payload.
fn failure(payload: Value) -> YieldError {
    let mut class = None;
    let mut message = None;
    if let Value::Map(pairs) = payload {
        for (key, value) in pairs {
            if let (Value::Str(key), Value::Str(text)) = (key, value) {
                match key.as_str() {
                    "class" => class = Some(text),
                    "message" => message = Some(text),
                    _ => {}
                }
            }
        }
    }
    YieldError::Failure {
        class: class.unwrap_or_else(|| "RuntimeError".into()),
        message: message.unwrap_or_else(|| "yield error".into()),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use kobako_codec::codec::Encode as _;
    use kobako_codec::transport::TAG_ERROR;
    use kobako_runtime::error::Trap;

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
        fn yield_block(&mut self, args: &[u8]) -> Result<Vec<u8>, Trap> {
            self.sent.push(args.to_vec());
            self.responses.pop_front().expect("script exhausted")
        }
    }

    fn response(tag: u8, value: Value) -> Vec<u8> {
        Yield { tag, value }.encode().unwrap()
    }

    #[test]
    fn call_ships_args_as_one_msgpack_array_and_returns_the_ok_value() {
        let mut channel = Scripted::new(vec![Ok(response(TAG_OK, Value::Int(42)))]);
        let mut block = Yielder::new(&mut channel);
        let value = block.call(&[Value::Int(21)]).unwrap();
        assert_eq!(value, Value::Int(42));
        // msgpack fixarray of one element: 0x91, then int 21 (0x15).
        assert_eq!(channel.sent, vec![vec![0x91, 0x15]]);
    }

    #[test]
    fn break_records_the_value_and_stops_re_entering_the_guest() {
        let mut channel = Scripted::new(vec![Ok(response(TAG_BREAK, Value::Sym("stop".into())))]);
        let mut block = Yielder::new(&mut channel);
        assert_eq!(block.call(&[]), Err(YieldError::Break));
        assert_eq!(block.call(&[]), Err(YieldError::Break));
        assert_eq!(block.into_break(), Some(Value::Sym("stop".into())));
        assert_eq!(
            channel.sent.len(),
            1,
            "a broken Yielder must not yield again"
        );
    }

    #[test]
    fn error_tag_surfaces_the_class_and_message() {
        let payload = Value::Map(vec![
            (
                Value::Str("class".into()),
                Value::Str("LocalJumpError".into()),
            ),
            (Value::Str("message".into()), Value::Str("boom".into())),
        ]);
        let mut channel = Scripted::new(vec![Ok(response(TAG_ERROR, payload))]);
        let mut block = Yielder::new(&mut channel);
        assert_eq!(
            block.call(&[]),
            Err(YieldError::Failure {
                class: "LocalJumpError".into(),
                message: "boom".into(),
            })
        );
    }

    #[test]
    fn error_tag_with_a_non_map_payload_falls_back_to_the_defaults() {
        let mut channel = Scripted::new(vec![Ok(response(TAG_ERROR, Value::Nil))]);
        let mut block = Yielder::new(&mut channel);
        assert_eq!(
            block.call(&[]),
            Err(YieldError::Failure {
                class: "RuntimeError".into(),
                message: "yield error".into(),
            })
        );
    }

    #[test]
    fn trap_during_re_entry_aborts() {
        let mut channel = Scripted::new(vec![Err(Trap::Timeout("deadline".into()))]);
        let mut block = Yielder::new(&mut channel);
        assert!(matches!(block.call(&[]), Err(YieldError::Aborted(_))));
    }

    #[test]
    fn malformed_response_bytes_abort() {
        let mut channel = Scripted::new(vec![Ok(vec![0x03, 0xc0])]);
        let mut block = Yielder::new(&mut channel);
        assert!(matches!(block.call(&[]), Err(YieldError::Aborted(_))));
    }

    #[test]
    fn every_yield_error_folds_to_a_runtime_fault() {
        let failure = YieldError::Failure {
            class: "LocalJumpError".into(),
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
