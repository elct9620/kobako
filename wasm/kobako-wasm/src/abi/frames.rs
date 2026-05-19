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

/// Decode Frame 3 snippets: `[{name, kind, body}, ...]`. `kind` must be
/// `"source"` in this revision (docs/wire-codec.md § Invocation
/// channels); other values are rejected as wire violations. Pure
/// parser — host-buildable for unit testing.
pub(super) fn decode_snippets(bytes: &[u8]) -> Option<Vec<(String, String)>> {
    let mut dec = Decoder::new(bytes);
    let outer = dec.read_value().ok()?;
    let outer_arr = match outer {
        Value::Array(items) => items,
        _ => return None,
    };
    let mut entries = Vec::with_capacity(outer_arr.len());
    for item in outer_arr {
        let pairs = match item {
            Value::Map(p) => p,
            _ => return None,
        };
        let mut name: Option<String> = None;
        let mut kind: Option<String> = None;
        let mut body: Option<String> = None;
        for (k, v) in pairs {
            let key = match k {
                Value::Str(s) => s,
                _ => return None,
            };
            let value = match v {
                Value::Str(s) => s,
                _ => return None,
            };
            match key.as_str() {
                "name" => name = Some(value),
                "kind" => kind = Some(value),
                "body" => body = Some(value),
                _ => {}
            }
        }
        let name = name?;
        let kind = kind?;
        let body = body?;
        if kind != "source" {
            return None;
        }
        entries.push((name, body));
    }
    Some(entries)
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
            vec![("Greeter".to_string(), "class Greeter; end".to_string())]
        );
    }

    #[test]
    fn decode_snippets_rejects_unknown_kind() {
        let bytes = encode(&Value::Array(vec![Value::Map(vec![
            (Value::Str("name".into()), Value::Str("Greeter".into())),
            (Value::Str("kind".into()), Value::Str("bytecode".into())),
            (Value::Str("body".into()), Value::Str("...".into())),
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
