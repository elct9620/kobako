//! Frame 3 snippet decoding — the mruby-shaped payload semantics.
//!
//! The channel mechanics (length-prefixed frame reader, Frame 1
//! preamble) live in `kobako_core::frames`; the snippet entry kinds
//! decoded here are mruby-specific — `"source"` carries mruby source
//! text, `"bytecode"` carries RITE bytes — so they stay with the
//! mruby implementation. See docs/wire-codec.md § Invocation channels.

use kobako_codec::codec::{Decoder, Value};

/// A decoded Frame 3 snippet entry — either a `code:` form source
/// snippet carrying its compile-time name and UTF-8 body, or a
/// `binary:` form bytecode blob whose name lives in the RITE
/// `debug_info` section (docs/wire-codec.md § Invocation channels).
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Snippet {
    /// Source form: `name` writes the compile context filename;
    /// `body` is UTF-8 mruby source.
    Source { name: String, body: String },
    /// Bytecode form: `body` is RITE bytecode bytes. The host does not
    /// extract the snippet's canonical name on this path — the guest
    /// reads it from the bytecode's embedded `debug_info` at load time.
    Bytecode { body: Vec<u8> },
}

/// Decode Frame 3 snippets. The shape is `[entry, ...]` where each
/// entry is a msgpack map keyed by `kind`:
///
///   * `"source"` — `{name: <str>, kind: "source", body: <str>}`
///   * `"bytecode"` — `{kind: "bytecode", body: <bin>}` (no `name`)
///
/// Any other `kind` value is a wire violation (docs/wire-codec.md
/// § Invocation channels). Pure parser — host-buildable for unit
/// testing.
pub(super) fn decode_snippets(bytes: &[u8]) -> Option<Vec<Snippet>> {
    let mut dec = Decoder::new(bytes);
    let outer = dec.read_only_value().ok()?;
    let outer_arr = match outer {
        Value::Array(items) => items,
        _ => return None,
    };
    let mut entries = Vec::with_capacity(outer_arr.len());
    for item in outer_arr {
        entries.push(decode_snippet_entry(item)?);
    }
    Some(entries)
}

/// Decode a single Frame 3 entry. Splits the entry map by `kind` and
/// dispatches to the per-kind builder; unknown kinds and shape
/// mismatches return `None` so the caller can fail the frame as a wire
/// violation.
fn decode_snippet_entry(item: Value) -> Option<Snippet> {
    let pairs = match item {
        Value::Map(p) => p,
        _ => return None,
    };
    let mut name: Option<String> = None;
    let mut kind: Option<String> = None;
    let mut body_str: Option<String> = None;
    let mut body_bin: Option<Vec<u8>> = None;
    for (k, v) in pairs {
        let key = match k {
            Value::Str(s) => s,
            _ => return None,
        };
        match (key.as_str(), v) {
            ("name", Value::Str(s)) => name = Some(s),
            ("kind", Value::Str(s)) => kind = Some(s),
            ("body", Value::Str(s)) => body_str = Some(s),
            ("body", Value::Bin(b)) => body_bin = Some(b),
            ("name" | "kind" | "body", _) => return None,
            _ => {}
        }
    }
    match kind?.as_str() {
        "source" => Some(Snippet::Source {
            name: name?,
            body: body_str?,
        }),
        "bytecode" => {
            // The wire contract (docs/wire-codec.md § Invocation
            // channels) reserves the `name` field for the source form.
            // A bytecode entry carrying it is a host bug — the
            // canonical name lives in the IREP's debug_info — so
            // surface the violation rather than silently dropping the
            // stray field.
            if name.is_some() {
                return None;
            }
            Some(Snippet::Bytecode { body: body_bin? })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kobako_codec::codec::Encoder;

    fn encode(v: &Value) -> Vec<u8> {
        let mut enc = Encoder::new();
        enc.write_value(v).unwrap();
        enc.into_bytes()
    }

    #[test]
    fn decode_snippets_accepts_source_kind() {
        let bytes = encode(&Value::Array(vec![Value::Map(vec![
            (Value::Str("name".into()), Value::Str("Greeter".into())),
            (Value::Str("kind".into()), Value::Str("source".into())),
            (
                Value::Str("body".into()),
                Value::Str("class Greeter; end".into()),
            ),
        ])]));
        let out = decode_snippets(&bytes).unwrap();
        assert_eq!(
            out,
            vec![Snippet::Source {
                name: "Greeter".into(),
                body: "class Greeter; end".into()
            }]
        );
    }

    #[test]
    fn decode_snippets_accepts_bytecode_kind() {
        let body_bytes = vec![0x52, 0x49, 0x54, 0x45]; // "RITE" magic
        let bytes = encode(&Value::Array(vec![Value::Map(vec![
            (Value::Str("kind".into()), Value::Str("bytecode".into())),
            (Value::Str("body".into()), Value::Bin(body_bytes.clone())),
        ])]));
        let out = decode_snippets(&bytes).unwrap();
        assert_eq!(out, vec![Snippet::Bytecode { body: body_bytes }]);
    }

    #[test]
    fn decode_snippets_rejects_unknown_kind() {
        let bytes = encode(&Value::Array(vec![Value::Map(vec![
            (Value::Str("name".into()), Value::Str("Greeter".into())),
            (Value::Str("kind".into()), Value::Str("unknown".into())),
            (Value::Str("body".into()), Value::Str("...".into())),
        ])]));
        assert!(decode_snippets(&bytes).is_none());
    }

    #[test]
    fn decode_snippets_rejects_bytecode_body_as_str() {
        // Bytecode entries must use msgpack bin; str is a wire violation.
        let bytes = encode(&Value::Array(vec![Value::Map(vec![
            (Value::Str("kind".into()), Value::Str("bytecode".into())),
            (Value::Str("body".into()), Value::Str("RITE".into())),
        ])]));
        assert!(decode_snippets(&bytes).is_none());
    }

    #[test]
    fn decode_snippets_rejects_bytecode_carrying_name_field() {
        // docs/wire-codec.md § Invocation channels reserves `name` for
        // the source form. A bytecode entry that ships with a name key
        // is a host bug — the canonical name lives in the IREP's
        // debug_info — and must surface as a wire violation rather
        // than be silently accepted.
        let body_bytes = vec![0x52, 0x49, 0x54, 0x45];
        let bytes = encode(&Value::Array(vec![Value::Map(vec![
            (Value::Str("name".into()), Value::Str("Helper".into())),
            (Value::Str("kind".into()), Value::Str("bytecode".into())),
            (Value::Str("body".into()), Value::Bin(body_bytes)),
        ])]));
        assert!(decode_snippets(&bytes).is_none());
    }

    #[test]
    fn decode_snippets_accepts_empty_array() {
        let bytes = encode(&Value::Array(Vec::new()));
        let out = decode_snippets(&bytes).unwrap();
        assert!(out.is_empty());
    }
}
