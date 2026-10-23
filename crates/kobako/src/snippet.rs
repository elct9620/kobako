//! Preloaded snippets: the per-Sandbox replay table the Preamble carries
//! to the guest.
//!
//! The SDK twin of the Ruby gem's `Kobako::Catalog::Snippets`: an
//! insertion-ordered table sealed together with the Service registry,
//! replayed into the fresh guest before per-invocation source or
//! entrypoint resolution. Source entries carry their canonical
//! backtrace name; bytecode entries stay opaque — their name, when
//! present, lives in the RITE `debug_info` the guest reads at load
//! time.

use kobako_runtime::runtime::Runtime;
use kobako_transport::envelope::{self, Preamble, Snippet};

use crate::error::Error;

/// Insertion-ordered snippet table for one Sandbox.
#[derive(Default)]
pub(crate) struct Snippets {
    table: envelope::Snippets,
}

impl Snippets {
    /// A duplicate name is refused, so backtrace attribution stays
    /// unambiguous.
    pub(crate) fn register_source(&mut self, name: &str, body: &str) -> Result<(), Error> {
        if !constant_name(name) {
            return Err(Error::Argument(format!(
                "snippet name must be a Ruby constant name (got {name:?})"
            )));
        }
        let duplicate = self
            .table
            .entries
            .iter()
            .any(|entry| matches!(entry, Snippet::Source { name: taken, .. } if taken == name));
        if duplicate {
            return Err(Error::Argument(format!(
                "snippet {name:?} already preloaded"
            )));
        }
        self.table.entries.push(Snippet::Source {
            name: name.to_string(),
            body: body.to_string(),
        });
        Ok(())
    }

    /// Recorded verbatim; the guest validates it at first replay.
    pub(crate) fn register_binary(&mut self, body: Vec<u8>) {
        self.table.entries.push(Snippet::Bytecode { body });
    }

    pub(crate) fn table(&self) -> &envelope::Snippets {
        &self.table
    }

    /// Has `runtime` compile the source entries under `preamble`, which
    /// carries this table, and puts each bytecode it answers in place of
    /// its source, so every invocation after replays it instead of
    /// compiling again. When nothing comes back the source stays.
    pub(crate) fn compile(&mut self, runtime: &dyn Runtime, preamble: &Preamble) {
        let has_source = self
            .table
            .entries
            .iter()
            .any(|entry| matches!(entry, Snippet::Source { .. }));
        if !has_source {
            return;
        }
        let Some(bytecodes) = runtime.compile(preamble) else {
            return;
        };
        let sources = self
            .table
            .entries
            .iter_mut()
            .filter(|entry| matches!(entry, Snippet::Source { .. }));
        for (entry, body) in sources.zip(bytecodes) {
            *entry = Snippet::Bytecode { body };
        }
    }
}

/// The guest interns every constant name, and mruby holds a symbol of at
/// most this many bytes.
const SYMBOL_MAX_LEN: usize = 65_534;

pub(crate) fn constant_name(name: &str) -> bool {
    let mut chars = name.chars();
    name.len() <= SYMBOL_MAX_LEN
        && chars.next().is_some_and(|c| c.is_ascii_uppercase())
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    // Replay order is the property the guest depends on.
    #[test]
    fn the_table_carries_source_and_binary_entries_in_insertion_order() {
        let mut snippets = Snippets::default();
        snippets.register_source("Helper", "X = 1").unwrap();
        snippets.register_binary(vec![0x01, 0x02]);
        assert_eq!(
            snippets.table(),
            &envelope::Snippets {
                entries: vec![
                    Snippet::Source {
                        name: "Helper".into(),
                        body: "X = 1".into()
                    },
                    Snippet::Bytecode {
                        body: vec![0x01, 0x02]
                    },
                ]
            },
            "a mixed snippet table must reach the runtime in registration order"
        );
    }

    // @behavior S-085
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

    // The bound sits where mruby stops interning, so the longest name it
    // holds passes and one byte more does not.
    #[test]
    fn constant_name_stops_at_the_longest_symbol() {
        let longest = format!("A{}", "b".repeat(SYMBOL_MAX_LEN - 1));
        assert!(
            constant_name(&longest),
            "a {SYMBOL_MAX_LEN}-byte name must pass"
        );
        assert!(
            !constant_name(&format!("{longest}b")),
            "a name one byte past the symbol limit must be refused"
        );
    }

    // @behavior S-194
    #[test]
    fn register_source_refuses_a_name_past_the_symbol_limit() {
        let name = format!("A{}", "b".repeat(SYMBOL_MAX_LEN));
        let refused = Snippets::default().register_source(&name, "X = 1");
        assert!(
            matches!(&refused, Err(Error::Argument(message)) if message.contains("constant name")),
            "a snippet name one byte past the symbol limit through register_source must be refused, \
             naming the constraint"
        );
    }

    // @behavior S-086
    #[test]
    fn register_source_rejects_a_duplicate_name() {
        let mut snippets = Snippets::default();
        snippets.register_source("Helper", "X = 1").unwrap();
        assert!(matches!(
            snippets.register_source("Helper", "Y = 2"),
            Err(Error::Argument(_))
        ));
    }

    // @behavior S-120
    #[test]
    fn register_binary_entries_stay_anonymous_and_may_repeat() {
        let mut snippets = Snippets::default();
        snippets.register_binary(vec![0x01]);
        snippets.register_binary(vec![0x01]);
        assert_eq!(snippets.table().entries.len(), 2);
    }
}
