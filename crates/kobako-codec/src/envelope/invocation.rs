//! The host→guest invocation envelopes — guest side.
//!
//! The guest decodes all three: a Run on the command buffer, and the two
//! stdin frames every entry point consumes before its verb-specific work.

use super::codec::{
    expect_end, put_bytes, put_list, put_u32, rest, take_bytes, take_text, take_text_list,
    take_u32, take_u8,
};
use super::Error;

const SNIPPET_SOURCE: u8 = 0;
const SNIPPET_BYTECODE: u8 = 1;

/// One `#run` invocation: which top-level constant, and its arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    pub entrypoint: String,
    pub payload: Vec<u8>,
}

impl Run {
    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        let mut at = 0usize;
        let entrypoint = take_text(bytes, &mut at)?.to_owned();
        Ok(Run {
            entrypoint,
            payload: rest(bytes, &at).to_vec(),
        })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        put_bytes(&mut out, self.entrypoint.as_bytes());
        out.extend_from_slice(&self.payload);
        out
    }
}

/// Frame 1 — the bound constant paths the guest installs proxies from.
/// Always present; an empty list means no Service is bound.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Preamble {
    pub paths: Vec<String>,
}

impl Preamble {
    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        let mut at = 0usize;
        let paths = take_text_list(bytes, &mut at)?;
        expect_end(bytes, &at)?;
        Ok(Preamble { paths })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        put_list(&mut out, &self.paths);
        out
    }
}

/// One preloaded snippet. Source carries the filename the guest compiles
/// under; bytecode does not, because its filename — when it has one — lives
/// in the bytecode's own debug section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Snippet {
    Source { name: String, body: String },
    Bytecode { body: Vec<u8> },
}

/// Frame 3 — the preloaded snippets, in insertion order. Always present;
/// a zero count means nothing was preloaded.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Snippets {
    pub entries: Vec<Snippet>,
}

impl Snippets {
    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        let mut at = 0usize;
        let count = take_u32(bytes, &mut at)? as usize;
        // Each entry costs at least a kind byte, so a count past the bytes
        // left cannot be satisfied; refusing it bounds the allocation.
        if count > bytes.len().saturating_sub(at) {
            return Err(Error("Frame 3 declares more entries than the frame holds"));
        }
        let mut entries = Vec::with_capacity(count);
        while entries.len() < count {
            entries.push(match take_u8(bytes, &mut at)? {
                SNIPPET_SOURCE => Snippet::Source {
                    name: take_text(bytes, &mut at)?.to_owned(),
                    body: take_text(bytes, &mut at)?.to_owned(),
                },
                SNIPPET_BYTECODE => Snippet::Bytecode {
                    body: take_bytes(bytes, &mut at)?.to_vec(),
                },
                _ => return Err(Error("Frame 3 snippet kind must be 0 or 1")),
            });
        }
        expect_end(bytes, &at)?;
        Ok(Snippets { entries })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        put_u32(&mut out, self.entries.len() as u32);
        for entry in &self.entries {
            match entry {
                Snippet::Source { name, body } => {
                    out.push(SNIPPET_SOURCE);
                    put_bytes(&mut out, name.as_bytes());
                    put_bytes(&mut out, body.as_bytes());
                }
                Snippet::Bytecode { body } => {
                    out.push(SNIPPET_BYTECODE);
                    put_bytes(&mut out, body);
                }
            }
        }
        out
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
            "a Run must survive a guest encode and decode unchanged"
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
    fn an_empty_preamble_round_trips() {
        let encoded = Preamble::default().encode();
        assert_eq!(
            Preamble::decode(&encoded),
            Ok(Preamble::default()),
            "a Sandbox with no bindings must send a present, empty Frame 1"
        );
    }

    #[test]
    fn a_preamble_round_trips_every_path() {
        let preamble = Preamble {
            paths: vec!["MyService::KV".into(), "File".into()],
        };
        let encoded = preamble.encode();
        assert_eq!(
            Preamble::decode(&encoded),
            Ok(preamble),
            "Frame 1 must carry every bound path in order"
        );
    }

    #[test]
    fn snippets_round_trip_both_kinds_in_order() {
        let snippets = Snippets {
            entries: vec![
                Snippet::Source {
                    name: "Helper".into(),
                    body: "def helper; end".into(),
                },
                Snippet::Bytecode {
                    body: vec![0x52, 0x49, 0x54, 0x45],
                },
            ],
        };
        let encoded = snippets.encode();
        assert_eq!(
            Snippets::decode(&encoded),
            Ok(snippets),
            "Frame 3 must carry source and bytecode entries in insertion order"
        );
    }

    #[test]
    fn an_empty_snippet_table_round_trips() {
        let encoded = Snippets::default().encode();
        assert_eq!(
            Snippets::decode(&encoded),
            Ok(Snippets::default()),
            "a Sandbox with no preloads must send a present, zero-count Frame 3"
        );
    }

    #[test]
    fn an_unknown_snippet_kind_is_refused() {
        let mut bytes = Vec::new();
        put_u32(&mut bytes, 1);
        bytes.push(9);
        assert!(
            Snippets::decode(&bytes).is_err(),
            "a Frame 3 snippet kind that is neither source nor bytecode must be rejected"
        );
    }

    #[test]
    fn a_snippet_count_the_frame_cannot_satisfy_is_refused() {
        assert!(
            Snippets::decode(&[0xff, 0xff, 0xff, 0xff]).is_err(),
            "a Frame 3 count larger than the frame must be rejected before any allocation"
        );
    }

    #[test]
    fn trailing_bytes_after_a_frame_are_refused() {
        let mut encoded = Preamble::default().encode();
        encoded.push(0);
        assert!(
            Preamble::decode(&encoded).is_err(),
            "bytes past Frame 1's last field must fail loudly as a framing desync"
        );
    }
}
