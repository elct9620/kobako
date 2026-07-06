//! Preloaded snippets: the per-Sandbox replay table and its Frame 3
//! wire image.
//!
//! The SDK twin of the Ruby gem's `Kobako::Catalog::Snippets`: an
//! insertion-ordered table sealed together with the Service registry,
//! replayed into the fresh guest before per-invocation source or
//! entrypoint resolution. Source entries carry their canonical
//! backtrace name; bytecode entries stay opaque — their name, when
//! present, lives in the RITE `debug_info` the guest reads at load
//! time.

use kobako_codec::codec::{Encoder, Value};

use crate::error::Error;

/// One preloaded snippet in its registered form.
enum Snippet {
    Source { name: String, body: String },
    Binary { body: Vec<u8> },
}

/// Insertion-ordered snippet table for one Sandbox.
#[derive(Default)]
pub(crate) struct Snippets {
    entries: Vec<Snippet>,
}

impl Snippets {
    /// Register a source-form snippet. The name is the snippet's
    /// canonical identity — it must be a Ruby constant name and may
    /// not duplicate an already-registered source entry, so backtrace
    /// attribution stays unambiguous.
    pub(crate) fn register_source(&mut self, name: &str, body: &str) -> Result<(), Error> {
        if !constant_name(name) {
            return Err(Error::Argument(format!(
                "snippet name must be a Ruby constant name (got {name:?})"
            )));
        }
        let duplicate = self
            .entries
            .iter()
            .any(|entry| matches!(entry, Snippet::Source { name: taken, .. } if taken == name));
        if duplicate {
            return Err(Error::Argument(format!(
                "snippet {name:?} already preloaded"
            )));
        }
        self.entries.push(Snippet::Source {
            name: name.to_string(),
            body: body.to_string(),
        });
        Ok(())
    }

    /// Register a binary-form snippet: RITE bytecode recorded verbatim,
    /// structurally validated by the guest at first replay.
    pub(crate) fn register_binary(&mut self, body: Vec<u8>) {
        self.entries.push(Snippet::Binary { body });
    }

    /// Encode the Frame 3 snippet-replay payload: a msgpack array of
    /// entry maps in insertion order, empty but present when nothing is
    /// preloaded (docs/wire-codec.md § Invocation channels).
    pub(crate) fn frame(&self) -> Vec<u8> {
        let entries = self
            .entries
            .iter()
            .map(|entry| match entry {
                Snippet::Source { name, body } => Value::Map(vec![
                    (Value::Str("name".into()), Value::Str(name.clone())),
                    (Value::Str("kind".into()), Value::Str("source".into())),
                    (Value::Str("body".into()), Value::Str(body.clone())),
                ]),
                Snippet::Binary { body } => Value::Map(vec![
                    (Value::Str("kind".into()), Value::Str("bytecode".into())),
                    (Value::Str("body".into()), Value::Bin(body.clone())),
                ]),
            })
            .collect();
        let mut encoder = Encoder::new();
        encoder
            .write_value(&Value::Array(entries))
            .expect("a str/bin snippet table always encodes");
        encoder.into_bytes()
    }
}

/// Ruby constant-name check (`/\A[A-Z]\w*\z/`), shared by snippet
/// registration and the `run` entrypoint pre-flight.
pub(crate) fn constant_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_uppercase())
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_encodes_source_and_binary_entries_in_insertion_order() {
        let mut snippets = Snippets::default();
        snippets.register_source("Helper", "X = 1").unwrap();
        snippets.register_binary(vec![0x01, 0x02]);
        // The wire image the Ruby host emits for the same table
        // (`Catalog::Snippets#encode` with one source + one binary entry).
        let expected: Vec<u8> = [
            "92", // fixarray 2
            "83a46e616d65a648656c706572a46b696e64a6736f75726365a4626f6479a558203d2031",
            "82a46b696e64a862797465636f6465a4626f6479c4020102",
        ]
        .concat()
        .as_bytes()
        .chunks(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect();
        assert_eq!(snippets.frame(), expected);
    }

    #[test]
    fn empty_table_frame_is_the_explicit_empty_array() {
        assert_eq!(Snippets::default().frame(), vec![0x90]);
    }

    #[test]
    fn register_source_rejects_a_non_constant_name() {
        for name in ["lower", "1X", "X::Y", "", "Na-me"] {
            let mut snippets = Snippets::default();
            assert!(
                matches!(
                    snippets.register_source(name, "X = 1"),
                    Err(Error::Argument(_))
                ),
                "{name:?} must be rejected"
            );
        }
    }

    #[test]
    fn register_source_rejects_a_duplicate_name() {
        let mut snippets = Snippets::default();
        snippets.register_source("Helper", "X = 1").unwrap();
        assert!(matches!(
            snippets.register_source("Helper", "Y = 2"),
            Err(Error::Argument(_))
        ));
    }

    #[test]
    fn register_binary_entries_stay_anonymous_and_may_repeat() {
        let mut snippets = Snippets::default();
        snippets.register_binary(vec![0x01]);
        snippets.register_binary(vec![0x01]);
        assert_eq!(snippets.entries.len(), 2);
    }
}
