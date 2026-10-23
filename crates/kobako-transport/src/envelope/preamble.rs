//! The Preamble — what an invocation reads before its entry runs: the
//! paths the host bound, the snippets it preloaded, and the flags it set.
//!
//! It is a counted list of records rather than fixed positions, so a later
//! kind joins as a registry entry instead of a new position every reader
//! must expect. Each record says whether a reader that does not know its
//! kind may skip it, which keeps a restriction from lapsing silently on an
//! older Guest Binary.

use super::bytes::{Reader, Writer};
use super::{tag, DecodeError};

const SNIPPET_SOURCE: u8 = 0;
const SNIPPET_BYTECODE: u8 = 1;

const ADVISORY: u8 = 0;
const CRITICAL: u8 = 1;

/// What the host sets an invocation up with, read by the guest before any
/// entry runs. A later record kind becomes a field here, so it is built
/// from `Default` rather than a literal.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct Preamble {
    pub bindings: Bindings,
    pub snippets: Snippets,
    pub flags: Vec<Flag>,
}

/// The bound constant paths the guest installs proxies from. An empty
/// list means no Service is bound.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Bindings {
    pub paths: Vec<String>,
}

/// One preloaded snippet. Source carries the filename the guest compiles
/// under; bytecode does not, because its filename — when it has one — lives
/// in the bytecode's own debug section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Snippet {
    Source { name: String, body: String },
    Bytecode { body: Vec<u8> },
}

/// The preloaded snippets, in insertion order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Snippets {
    pub entries: Vec<Snippet>,
}

/// A switch the host sets for this invocation's VM, by name. An empty
/// `value` is a plain switch. A Guest Binary that does not recognise the
/// name refuses the invocation when the flag is `critical`, and skips it
/// otherwise.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Flag {
    pub name: String,
    pub value: String,
    pub critical: bool,
}

impl Preamble {
    /// A record of a kind this reader does not know is skipped when
    /// advisory and refused when critical. Bindings and snippets each
    /// appear at most once; an absent one reads as empty.
    pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(bytes);
        let count = reader.u32()? as usize;
        // Each record costs at least six bytes, its two leading bytes and a
        // body length, so a count past the bytes left can never be
        // satisfied; refusing it bounds the work.
        if count > reader.remaining().len() {
            return Err(DecodeError::new(
                "Preamble declares more records than it holds",
            ));
        }
        let mut bindings = None;
        let mut snippets = None;
        let mut flags = Vec::new();
        for _ in 0..count {
            let critical = match reader.u8()? {
                ADVISORY => false,
                CRITICAL => true,
                _ => return Err(DecodeError::new("Preamble criticality must be 0 or 1")),
            };
            let kind = reader.u8()?;
            let body = reader.bytes()?;
            match kind {
                tag::BINDINGS => once(&mut bindings, decode_bindings(body)?)?,
                tag::SNIPPETS => once(&mut snippets, decode_snippets(body)?)?,
                tag::FLAG => flags.push(decode_flag(body, critical)?),
                kind if tag::is_assigned(kind) => {
                    return Err(DecodeError::new(
                        "Preamble record kind is assigned to another message",
                    ))
                }
                _ if critical => {
                    return Err(DecodeError::new(
                        "Preamble carries a critical record of a kind this reader does not know",
                    ))
                }
                _ => {}
            }
        }
        reader.finish()?;
        Ok(Preamble {
            bindings: bindings.unwrap_or_default(),
            snippets: snippets.unwrap_or_default(),
            flags,
        })
    }

    /// Empty bindings and snippets are left out, since an absent record
    /// already reads as empty.
    pub fn encode(&self) -> Vec<u8> {
        let mut records: Vec<(bool, u8, Vec<u8>)> = Vec::new();
        if !self.bindings.paths.is_empty() {
            records.push((true, tag::BINDINGS, encode_bindings(&self.bindings)));
        }
        if !self.snippets.entries.is_empty() {
            records.push((true, tag::SNIPPETS, encode_snippets(&self.snippets)));
        }
        for flag in &self.flags {
            records.push((flag.critical, tag::FLAG, encode_flag(flag)));
        }
        let mut writer = Writer::new();
        writer.u32(records.len() as u32);
        for (critical, kind, body) in &records {
            writer
                .u8(if *critical { CRITICAL } else { ADVISORY })
                .u8(*kind)
                .bytes(body);
        }
        writer.into_bytes()
    }
}

fn once<T>(slot: &mut Option<T>, value: T) -> Result<(), DecodeError> {
    if slot.replace(value).is_some() {
        return Err(DecodeError::new(
            "Preamble repeats a record that appears once",
        ));
    }
    Ok(())
}

fn decode_bindings(bytes: &[u8]) -> Result<Bindings, DecodeError> {
    let mut reader = Reader::new(bytes);
    let paths = reader.text_list()?;
    reader.finish()?;
    Ok(Bindings { paths })
}

fn encode_bindings(bindings: &Bindings) -> Vec<u8> {
    let mut writer = Writer::new();
    writer.list(&bindings.paths);
    writer.into_bytes()
}

fn decode_snippets(bytes: &[u8]) -> Result<Snippets, DecodeError> {
    let mut reader = Reader::new(bytes);
    let count = reader.u32()? as usize;
    // Each entry costs at least a kind byte and a `u32` length, so a count
    // past what the bytes left can hold is refused before it sizes the
    // allocation.
    if count > reader.remaining().len() / (1 + size_of::<u32>()) {
        return Err(DecodeError::new(
            "Preamble snippets declare more entries than they hold",
        ));
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
            _ => return Err(DecodeError::new("Preamble snippet kind must be 0 or 1")),
        });
    }
    reader.finish()?;
    Ok(Snippets { entries })
}

fn encode_snippets(snippets: &Snippets) -> Vec<u8> {
    let mut writer = Writer::new();
    writer.u32(snippets.entries.len() as u32);
    for entry in &snippets.entries {
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

fn decode_flag(bytes: &[u8], critical: bool) -> Result<Flag, DecodeError> {
    let mut reader = Reader::new(bytes);
    let name = reader.text()?.to_owned();
    let value = reader.text()?.to_owned();
    reader.finish()?;
    Ok(Flag {
        name,
        value,
        critical,
    })
}

fn encode_flag(flag: &Flag) -> Vec<u8> {
    let mut writer = Writer::new();
    writer
        .bytes(flag.name.as_bytes())
        .bytes(flag.value.as_bytes());
    writer.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flag(name: &str, value: &str, critical: bool) -> Flag {
        Flag {
            name: name.into(),
            value: value.into(),
            critical,
        }
    }

    fn record(critical: u8, kind: u8, body: &[u8]) -> Vec<u8> {
        let mut writer = Writer::new();
        writer.u8(critical).u8(kind).bytes(body);
        writer.into_bytes()
    }

    fn preamble_with(records: &[Vec<u8>]) -> Vec<u8> {
        let mut bytes = (records.len() as u32).to_be_bytes().to_vec();
        for record in records {
            bytes.extend_from_slice(record);
        }
        bytes
    }

    // @behavior WE-050
    #[test]
    fn an_empty_preamble_counts_no_records() {
        let encoded = Preamble::default().encode();
        assert_eq!(
            (encoded.as_slice(), Preamble::decode(&encoded)),
            (&[0u8, 0, 0, 0][..], Ok(Preamble::default())),
            "a Preamble with nothing set up through encode must write a zero count and read \
             back empty"
        );
    }

    // @behavior WE-051
    #[test]
    fn bindings_round_trip_every_path() {
        let preamble = Preamble {
            bindings: Bindings {
                paths: vec!["MyService::KV".into(), "File".into()],
            },
            ..Preamble::default()
        };
        assert_eq!(
            Preamble::decode(&preamble.encode()),
            Ok(preamble),
            "every bound path through a Preamble's encode and decode must arrive in order"
        );
    }

    // @behavior WE-052
    #[test]
    fn snippets_round_trip_both_kinds_in_order() {
        let preamble = Preamble {
            snippets: Snippets {
                entries: vec![
                    Snippet::Source {
                        name: "Helper".into(),
                        body: "def helper; end".into(),
                    },
                    Snippet::Bytecode {
                        body: vec![0x52, 0x49, 0x54, 0x45],
                    },
                ],
            },
            ..Preamble::default()
        };
        assert_eq!(
            Preamble::decode(&preamble.encode()),
            Ok(preamble),
            "source and bytecode snippets through a Preamble's encode and decode must arrive \
             in insertion order"
        );
    }

    // @behavior WE-053
    #[test]
    fn an_absent_record_reads_as_empty() {
        let bytes = preamble_with(&[record(0, 0x07, &encode_flag(&flag("f", "", false)))]);
        let preamble = Preamble::decode(&bytes).expect("a flag-only Preamble must decode");
        assert_eq!(
            (preamble.bindings, preamble.snippets),
            (Bindings::default(), Snippets::default()),
            "a Preamble carrying neither bindings nor snippets through decode must read both \
             as empty"
        );
    }

    // @behavior WE-054
    #[test]
    fn golden_layout_pins_the_record_list() {
        let preamble = Preamble {
            bindings: Bindings {
                paths: vec!["A".into()],
            },
            flags: vec![flag("n", "v", false)],
            ..Preamble::default()
        };
        assert_eq!(
            preamble.encode(),
            vec![
                0, 0, 0, 2, // record count
                1, 0x05, // critical, kind: bindings
                0, 0, 0, 9, // body length
                0, 0, 0, 1, // path count
                0, 0, 0, 1, b'A', //
                0, 0x07, // advisory, kind: flag
                0, 0, 0, 10, // body length
                0, 0, 0, 1, b'n', // name
                0, 0, 0, 1, b'v', // value
            ],
            "a Preamble through encode must write a count, then each record as its \
             criticality, its kind, and its length-prefixed body"
        );
    }

    // @behavior WE-055
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
            encode_snippets(&snippets),
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

    // @behavior WE-056
    #[test]
    fn an_unknown_snippet_kind_is_refused() {
        let bytes = preamble_with(&[record(1, 0x06, &[0, 0, 0, 1, 9])]);
        assert!(
            Preamble::decode(&bytes).is_err(),
            "a snippet kind that is neither source nor bytecode through Preamble::decode must \
             be refused"
        );
    }

    // @behavior WE-057
    #[test]
    fn a_count_the_preamble_cannot_satisfy_is_refused() {
        let records = [0xff, 0xff, 0xff, 0xff];
        let snippets = preamble_with(&[record(1, 0x06, &[0xff, 0xff, 0xff, 0xff])]);
        assert!(
            Preamble::decode(&records).is_err() && Preamble::decode(&snippets).is_err(),
            "a record or snippet count larger than what follows through Preamble::decode must \
             be refused before anything is allocated for it"
        );
    }

    // @behavior WE-058
    #[test]
    fn trailing_bytes_are_refused() {
        let mut past_the_list = Preamble::default().encode();
        past_the_list.push(0);
        let past_a_body = preamble_with(&[record(1, 0x05, &[0, 0, 0, 0, 0])]);
        assert!(
            Preamble::decode(&past_the_list).is_err() && Preamble::decode(&past_a_body).is_err(),
            "bytes past the last record or past a record body's last field through \
             Preamble::decode must fail loudly as a framing desync"
        );
    }

    // @behavior WE-077
    #[test]
    fn an_advisory_record_of_an_unknown_kind_is_skipped() {
        let bytes = preamble_with(&[
            record(0, 0x7e, b"anything"),
            record(
                1,
                0x05,
                &encode_bindings(&Bindings {
                    paths: vec!["A".into()],
                }),
            ),
        ]);
        assert_eq!(
            Preamble::decode(&bytes).map(|preamble| preamble.bindings.paths),
            Ok(vec!["A".to_string()]),
            "an advisory record of a kind the registry has not assigned through \
             Preamble::decode must be skipped, leaving the records around it intact"
        );
    }

    // @behavior WE-078
    #[test]
    fn a_critical_record_of_an_unknown_kind_is_refused() {
        let bytes = preamble_with(&[record(1, 0x7e, b"anything")]);
        assert!(
            Preamble::decode(&bytes).is_err(),
            "a critical record of a kind the registry has not assigned through \
             Preamble::decode must be refused"
        );
    }

    // @behavior WE-079
    #[test]
    fn a_record_kind_assigned_to_another_message_is_refused() {
        for kind in [0x00, 0x01, 0x10] {
            let bytes = preamble_with(&[record(0, kind, b"")]);
            assert!(
                Preamble::decode(&bytes).is_err(),
                "an advisory record of kind {kind:#04x}, which the registry assigns elsewhere, \
                 through Preamble::decode must be refused rather than skipped"
            );
        }
    }

    // @behavior WE-080
    #[test]
    fn a_repeated_record_is_refused() {
        let bindings = encode_bindings(&Bindings::default());
        let snippets = encode_snippets(&Snippets::default());
        for (kind, body) in [(0x05, bindings), (0x06, snippets)] {
            let bytes = preamble_with(&[record(1, kind, &body), record(1, kind, &body)]);
            assert!(
                Preamble::decode(&bytes).is_err(),
                "a second record of kind {kind:#04x} through Preamble::decode must be refused"
            );
        }
    }

    // @behavior WE-081
    #[test]
    fn flags_round_trip_with_their_criticality_in_order() {
        let preamble = Preamble {
            flags: vec![flag("strict", "", true), flag("limit", "8", false)],
            ..Preamble::default()
        };
        assert_eq!(
            Preamble::decode(&preamble.encode()),
            Ok(preamble),
            "flags through a Preamble's encode and decode must arrive in order, each with its \
             value and criticality"
        );
    }

    // @behavior WE-082
    #[test]
    fn a_criticality_that_is_neither_value_is_refused() {
        let bytes = preamble_with(&[record(2, 0x07, &encode_flag(&flag("f", "", false)))]);
        assert!(
            Preamble::decode(&bytes).is_err(),
            "a criticality byte other than 0 or 1 through Preamble::decode must be refused"
        );
    }

    // @behavior WE-083
    #[test]
    fn records_read_the_same_in_any_order() {
        let bindings = record(
            1,
            0x05,
            &encode_bindings(&Bindings {
                paths: vec!["A".into()],
            }),
        );
        let flag = record(0, 0x07, &encode_flag(&flag("f", "", false)));
        assert_eq!(
            Preamble::decode(&preamble_with(&[flag.clone(), bindings.clone()])),
            Preamble::decode(&preamble_with(&[bindings, flag])),
            "the same records in another order through Preamble::decode must read as the same \
             Preamble"
        );
    }
}
