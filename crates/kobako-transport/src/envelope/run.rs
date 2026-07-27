//! The Run envelope — the host→guest request one `#run` invocation carries.
//!
//! Run is Call's reverse-direction sibling — `entrypoint` routes it, the
//! payload feeds it. It carries no `method` because the entrypoint is
//! invoked through its own `#call`, and no `block_given` because `#run`
//! supplies no block. It rides the command buffer rather than a stdin
//! frame, so its length reaches the guest as an export argument.

use super::bytes::{Reader, Writer};
use super::Error;

/// One `#run` invocation: which top-level constant, and its arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    pub entrypoint: String,
    pub payload: Vec<u8>,
}

impl Run {
    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        let mut reader = Reader::new(bytes);
        let entrypoint = reader.text()?.to_owned();
        Ok(Run {
            entrypoint,
            payload: reader.remaining().to_vec(),
        })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut writer = Writer::new();
        writer
            .bytes(self.entrypoint.as_bytes())
            .remainder(&self.payload);
        writer.into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_run_round_trips() {
        let run = Run {
            entrypoint: "Entry".into(),
            payload: vec![0x92, 0x90, 0x80],
        };
        let encoded = run.encode();
        assert_eq!(
            Run::decode(&encoded),
            Ok(run),
            "a Run must survive a host encode and decode unchanged"
        );
    }

    #[test]
    fn a_run_with_no_arguments_round_trips() {
        let run = Run {
            entrypoint: "Entry".into(),
            payload: Vec::new(),
        };
        let encoded = run.encode();
        assert_eq!(
            Run::decode(&encoded),
            Ok(run),
            "a Run with an empty payload must decode as empty, not as a truncation"
        );
    }

    #[test]
    fn golden_layout_pins_the_run_field_order() {
        let run = Run {
            entrypoint: "E".into(),
            payload: vec![0xc0],
        };
        assert_eq!(
            run.encode(),
            vec![
                0, 0, 0, 1, b'E', // entrypoint
                0xc0, // payload remainder
            ],
            "the Run byte layout must stay fixed for both peers to agree"
        );
    }
}
