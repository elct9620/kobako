//! Stdin frame I/O and Frame 1 / Frame 3 decoders.
//!
//! Stdin frame format: 4-byte big-endian u32 length prefix + payload
//! bytes. Frame 1 (preamble) and Frame 3 (snippets) parsers are shared
//! across [`super::eval`] and [`super::run`]; the entry-specific
//! Frame 2 (user source for `__kobako_eval`) is read inline by the
//! eval entry. See docs/wire-codec.md § Invocation channels.

use crate::codec::{Decoder, Value};

/// Read one length-prefixed stdin frame. Returns `None` on EOF or short
/// read; callers turn that into a Panic envelope.
#[cfg(target_arch = "wasm32")]
pub(super) fn read_frame() -> Option<Vec<u8>> {
    use std::io::Read;
    let mut len_buf = [0u8; crate::FRAME_LEN_SIZE];
    let mut stdin = std::io::stdin().lock();
    stdin.read_exact(&mut len_buf).ok()?;
    let len = u32::from_be_bytes(len_buf) as usize;
    let mut payload = vec![0u8; len];
    stdin.read_exact(&mut payload).ok()?;
    Some(payload)
}

/// Decode the Frame 1 preamble: `[["Name", ["MemberA", ...]], ...]`.
/// Each entry binds a Group name to its Member name list. Pure parser —
/// host-buildable so the decoder can be unit-tested outside the wasm
/// target.
pub(super) fn decode_preamble(bytes: &[u8]) -> Option<Vec<(String, Vec<String>)>> {
    let mut dec = Decoder::new(bytes);
    let outer = dec.read_value().ok()?;
    let outer_arr = match outer {
        Value::Array(items) => items,
        _ => return None,
    };
    let mut groups = Vec::with_capacity(outer_arr.len());
    for item in outer_arr {
        let pair = match item {
            Value::Array(p) if p.len() == 2 => p,
            _ => return None,
        };
        let group_name = match &pair[0] {
            Value::Str(s) => s.clone(),
            _ => return None,
        };
        let members_arr = match &pair[1] {
            Value::Array(m) => m,
            _ => return None,
        };
        let mut members = Vec::with_capacity(members_arr.len());
        for m in members_arr {
            match m {
                Value::Str(s) => members.push(s.clone()),
                _ => return None,
            }
        }
        groups.push((group_name, members));
    }
    Some(groups)
}

/// A decoded Frame 3 snippet entry — either a `code:` form source
/// snippet carrying its compile-time name and UTF-8 body, or a
/// `binary:` form bytecode blob whose name lives in the RITE
/// `debug_info` section (docs/wire-codec.md § Invocation channels).
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Snippet {
    /// Source form: `name` writes the compile ccontext filename;
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
    let outer = dec.read_value().ok()?;
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
    use crate::codec::Encoder;

    fn encode(v: &Value) -> Vec<u8> {
        let mut enc = Encoder::new();
        enc.write_value(v).unwrap();
        enc.into_bytes()
    }

    #[test]
    fn decode_preamble_accepts_well_formed_groups() {
        let bytes = encode(&Value::Array(vec![Value::Array(vec![
            Value::Str("KV".into()),
            Value::Array(vec![Value::Str("Get".into()), Value::Str("Set".into())]),
        ])]));
        let out = decode_preamble(&bytes).unwrap();
        assert_eq!(
            out,
            vec![("KV".to_string(), vec!["Get".into(), "Set".into()])]
        );
    }

    #[test]
    fn decode_preamble_rejects_non_array_outer() {
        let bytes = encode(&Value::Map(Vec::new()));
        assert!(decode_preamble(&bytes).is_none());
    }

    #[test]
    fn decode_preamble_rejects_non_string_group_name() {
        let bytes = encode(&Value::Array(vec![Value::Array(vec![
            Value::Int(1),
            Value::Array(Vec::new()),
        ])]));
        assert!(decode_preamble(&bytes).is_none());
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
