//! The invocation frames — Frame 1's bound constant paths and Frame 3's
//! preloaded snippets, the two both entry points read before running
//! anything. The numbering skips Frame 2 because the `#eval` user source
//! is raw UTF-8 with no envelope of its own.
//!
//! Both are mandatory-presence even when empty, so a reader never has to
//! tell an absent frame from an empty one, and each frame's length reaches
//! the guest from the channel's own prefix rather than from the envelope.

use super::bytes::{Reader, Writer};
use super::Error;

const SNIPPET_SOURCE: u8 = 0;
const SNIPPET_BYTECODE: u8 = 1;

/// Frame 1 — the bound constant paths the guest installs proxies from.
/// Always present; an empty list means no Service is bound.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Preamble {
    pub paths: Vec<String>,
}

impl Preamble {
    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        let mut reader = Reader::new(bytes);
        let paths = reader.text_list()?;
        reader.finish()?;
        Ok(Preamble { paths })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut writer = Writer::new();
        writer.list(&self.paths);
        writer.into_bytes()
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
        let mut reader = Reader::new(bytes);
        let count = reader.u32()? as usize;
        // Each entry costs at least a kind byte, so a count past the bytes
        // left cannot be satisfied; refusing it bounds the allocation.
        if count > reader.remaining().len() {
            return Err(Error("Frame 3 declares more entries than the frame holds"));
        }
        let mut entries = Vec::with_capacity(count);
        for _ in 0..count {
            entries.push(match reader.u8()? {
                SNIPPET_SOURCE => Snippet::Source {
                    name: reader.text()?.to_owned(),
                    body: reader.text()?.to_owned(),
                },
                SNIPPET_BYTECODE => Snippet::Bytecode {
                    body: reader.bytes()?.to_vec(),
                },
                _ => return Err(Error("Frame 3 snippet kind must be 0 or 1")),
            });
        }
        reader.finish()?;
        Ok(Snippets { entries })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut writer = Writer::new();
        writer.u32(self.entries.len() as u32);
        for entry in &self.entries {
            match entry {
                Snippet::Source { name, body } => {
                    writer
                        .u8(SNIPPET_SOURCE)
                        .bytes(name.as_bytes())
                        .bytes(body.as_bytes());
                }
                Snippet::Bytecode { body } => {
                    writer.u8(SNIPPET_BYTECODE).bytes(body);
                }
            }
        }
        writer.into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn golden_layout_pins_the_preamble_as_a_counted_list() {
        let preamble = Preamble {
            paths: vec!["A".into()],
        };
        assert_eq!(
            preamble.encode(),
            vec![
                0, 0, 0, 1, // path count
                0, 0, 0, 1, b'A',
            ],
            "Frame 1 must be a counted list of paths, so the count precedes the first entry"
        );
    }

    #[test]
    fn golden_layout_pins_the_snippet_entry_shape() {
        let snippets = Snippets {
            entries: vec![
                Snippet::Source {
                    name: "N".into(),
                    body: "b".into(),
                },
                Snippet::Bytecode { body: vec![0x52] },
            ],
        };
        assert_eq!(
            snippets.encode(),
            vec![
                0, 0, 0, 2, // entry count
                0, // kind: source
                0, 0, 0, 1, b'N', // name, source only
                0, 0, 0, 1, b'b', // body
                1,    // kind: bytecode
                0, 0, 0, 1, 0x52, // body, no name
            ],
            "a source entry must encode as kind byte 0 with a name and a bytecode entry as \
             kind byte 1 with none, so the two stay distinguishable by their tag alone"
        );
    }

    #[test]
    fn an_unknown_snippet_kind_is_refused() {
        let bytes = {
            let mut w = Writer::new();
            w.u32(1).u8(9);
            w.into_bytes()
        };
        assert!(
            Snippets::decode(&bytes).is_err(),
            "a Frame 3 snippet kind that is neither source nor bytecode must be rejected"
        );
    }

    #[test]
    fn a_snippet_count_the_frame_cannot_satisfy_is_refused() {
        let bytes = [0xff, 0xff, 0xff, 0xff];
        assert!(
            Snippets::decode(&bytes).is_err(),
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
