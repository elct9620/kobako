//! The outcome position: a run's result read back as a value tree.

use kobako_codec::msgpack::codec::{Decoder, Value, MAX_PAYLOAD_VALUES};

use crate::error::Error;
use crate::execution::Execution;

impl Execution {
    /// The outcome decoded through the default payload codec, with every
    /// Handle in it checked live. A guest cannot fabricate a Handle, so
    /// an unknown id means a corrupted runtime and fails like a malformed
    /// value.
    ///
    /// It decodes per call, so a caller reading the value more than once
    /// holds onto what it gets.
    pub fn value(&self) -> Result<Value, Error> {
        let value = decode_value(self.payload().map_err(Clone::clone)?)?;
        self.require_live_handles(&value)?;
        Ok(value)
    }

    fn require_live_handles(&self, value: &Value) -> Result<(), Error> {
        match value {
            Value::Handle(id) => self
                .resolve(*id)
                .map(|_| ())
                .ok_or_else(|| Error::unknown_handle(*id)),
            Value::Array(items) => items.iter().try_for_each(|v| self.require_live_handles(v)),
            Value::Map(pairs) => pairs.iter().try_for_each(|(key, val)| {
                self.require_live_handles(key)?;
                self.require_live_handles(val)
            }),
            _ => Ok(()),
        }
    }
}

/// Attribution already framed the outcome, so a fault here is the
/// schema's: a sandbox-origin wire violation with the codec detail kept for
/// operator triage.
fn decode_value(body: &[u8]) -> Result<Value, Error> {
    Decoder::new(body)
        .with_value_budget(Some(MAX_PAYLOAD_VALUES))
        .read_sole_value()
        .map_err(Error::invalid_result)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use kobako_codec::msgpack::codec::Encoder;
    use kobako_runtime::snapshot::Capture;
    use kobako_transport::envelope::{Outcome, TRANSPORT_ERROR};

    use super::*;
    use crate::execution::{classify, Usage};

    // @behavior OC-006
    #[test]
    fn the_ok_arm_yields_payload_bytes_the_codec_reads_back() {
        let outcome = Outcome::Ok(Encoder::encode(&Value::Int(42)).unwrap());

        assert_eq!(
            decode_value(&classify(outcome, "eval").unwrap()).unwrap(),
            Value::Int(42),
            "an ok arm carrying an encodable value must read back through the codec unchanged"
        );
    }

    // @behavior OC-053
    #[test]
    fn a_value_naming_a_handle_never_issued_is_a_sandbox_failure() {
        let execution = Execution::new(
            Ok(Encoder::encode(&Value::Handle(5)).unwrap()),
            Arc::default(),
            Capture::default(),
            Capture::default(),
            Usage::default(),
        );

        assert!(
            matches!(execution.value(), Err(Error::Sandbox(_))),
            "Execution::value over a result naming a Handle id the invocation never issued \
             must fail as a Sandbox failure"
        );
    }

    // @behavior OC-003
    #[test]
    fn a_malformed_value_body_is_a_wire_violation_sandbox_error() {
        // The ok arm followed by a truncated msgpack str header.
        let result = classify(Outcome::Ok(vec![0xd9]), "eval").and_then(|body| decode_value(&body));

        assert!(
            matches!(result, Err(Error::Sandbox(f)) if f.name == TRANSPORT_ERROR),
            "an ok arm the codec cannot read must attribute to the wire-level error class"
        );
    }
}
