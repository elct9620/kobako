//! The outcome position: a run's result read back as a value tree.

use kobako_codec::msgpack::codec::{Decoder, Value};

use crate::error::{Error, Failure};
use crate::execution::Execution;

/// SPEC-pinned wire-level error class, carried as the attribution of
/// host-detected wire violations on both frontends.
const WIRE_ERROR_CLASS: &str = "Kobako::Transport::Error";

impl Execution {
    /// The outcome decoded through the default payload codec, with every
    /// Handle in it checked live. A guest cannot fabricate a Handle, so
    /// an unknown id means a corrupted runtime and fails like a malformed
    /// value.
    ///
    /// The decode happens here rather than at invocation because it is
    /// the one step that needs a schema; a host with its own reads
    /// `payload` instead. It runs per call, so a caller reading the value
    /// more than once holds onto what it gets.
    pub fn value(&self) -> Result<Value, Error> {
        let value = decode_value(self.payload().map_err(Clone::clone)?)?;
        self.require_live_handles(&value)?;
        Ok(value)
    }

    /// Consume the Execution and fold its outcome into a `Result` — the
    /// ergonomic path for a caller that wants the value and lets a guest
    /// failure propagate with `?`. Reach for the captures / `usage`
    /// before calling this, since it drops them.
    pub fn into_value(self) -> Result<Value, Error> {
        self.value()
    }

    fn require_live_handles(&self, value: &Value) -> Result<(), Error> {
        match value {
            Value::Handle(id) => self.resolve(*id).map(|_| ()).ok_or_else(|| {
                Error::Sandbox(Box::new(Failure {
                    class: "Kobako::SandboxError".into(),
                    message: format!("unknown Handle id: {id}"),
                    backtrace: Vec::new(),
                    available: Vec::new(),
                    diagnostic: None,
                }))
            }),
            Value::Array(items) => items.iter().try_for_each(|v| self.require_live_handles(v)),
            Value::Map(pairs) => pairs.iter().try_for_each(|(key, val)| {
                self.require_live_handles(key)?;
                self.require_live_handles(val)
            }),
            _ => Ok(()),
        }
    }
}

/// Read the Result arm's payload back. Attribution already framed the
/// outcome, so a fault here is the schema's: the bytes carry a value this
/// codec cannot read, which is a sandbox-origin wire violation with the
/// codec detail preserved for operator triage.
fn decode_value(body: &[u8]) -> Result<Value, Error> {
    Decoder::new(body)
        .read_only_value()
        .map_err(|err| wire_violation("Sandbox produced an invalid result value", &err))
}

fn wire_violation(message: &str, detail: &kobako_codec::msgpack::codec::Error) -> Error {
    Error::Sandbox(Box::new(Failure {
        class: WIRE_ERROR_CLASS.into(),
        message: message.into(),
        backtrace: Vec::new(),
        available: Vec::new(),
        diagnostic: Some(detail.to_string()),
    }))
}

#[cfg(test)]
mod tests {
    use kobako_codec::msgpack::codec::Encoder;
    use kobako_transport::envelope::Outcome;

    use super::*;
    use crate::outcome::classify;

    #[test]
    fn the_result_arm_yields_payload_bytes_the_codec_reads_back() {
        let bytes = Outcome::Result(Encoder::encode(&Value::Int(42)).unwrap()).encode();

        assert_eq!(
            decode_value(&classify(&bytes).unwrap()).unwrap(),
            Value::Int(42),
            "a Result arm carrying an encodable value must read back through the codec unchanged"
        );
    }

    #[test]
    fn a_malformed_value_body_is_a_wire_violation_sandbox_error() {
        // The Result arm followed by a truncated msgpack str header.
        let result =
            classify(&Outcome::Result(vec![0xd9]).encode()).and_then(|body| decode_value(&body));

        assert!(
            matches!(result, Err(Error::Sandbox(f)) if f.class == WIRE_ERROR_CLASS),
            "a Result arm the codec cannot read must attribute to the wire-level error class"
        );
    }
}
