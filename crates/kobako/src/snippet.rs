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

use kobako_runtime::envelope::{Snippet, Snippets as Frame};

use crate::error::Error;

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
        self.entries.push(Snippet::Bytecode { body });
    }

    /// Encode the Frame 3 snippet-replay payload in insertion order,
    /// empty but present when nothing is preloaded (docs/wire-codec.md
    /// § Invocation channels).
    pub(crate) fn frame(&self) -> Vec<u8> {
        Frame {
            entries: self.entries.clone(),
        }
        .encode()
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

    // Replay order is the property the guest depends on; read it back
    // through the envelope rather than pinning bytes the envelope's own
    // tests already own.
    #[test]
    fn the_frame_carries_source_and_binary_entries_in_insertion_order() {
        let mut snippets = Snippets::default();
        snippets.register_source("Helper", "X = 1").unwrap();
        snippets.register_binary(vec![0x01, 0x02]);
        assert_eq!(
            Frame::decode(&snippets.frame()),
            Ok(Frame {
                entries: vec![
                    Snippet::Source {
                        name: "Helper".into(),
                        body: "X = 1".into()
                    },
                    Snippet::Bytecode {
                        body: vec![0x01, 0x02]
                    },
                ]
            }),
            "a mixed snippet table must reach the guest in registration order"
        );
    }

    #[test]
    fn an_empty_table_sends_a_present_zero_count_frame() {
        assert_eq!(
            Frame::decode(&Snippets::default().frame()),
            Ok(Frame::default()),
            "a Sandbox with no preloads must send a present, zero-count Frame 3"
        );
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
