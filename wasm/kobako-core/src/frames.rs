//! Stdin invocation-channel frame mechanics.
//!
//! Every invocation entry point consumes length-prefixed stdin frames
//! (4-byte big-endian u32 length + payload — docs/wire-codec.md
//! § Invocation channels): `read_frame` is the channel reader and
//! `decode_preamble` parses Frame 1 (the host's Namespace Group /
//! Member name lists every dispatching guest installs proxies from).
//! Frame payload semantics that belong to a guest language — e.g. the
//! bundled guest's Frame 3 snippet kinds (mruby source / RITE
//! bytecode) — stay in the implementation crate; this module carries
//! only the language-neutral wire shapes.

use crate::codec::{Decoder, Value};

/// Read one length-prefixed stdin frame. Returns `None` on EOF or short
/// read; callers turn that into a Panic envelope.
pub fn read_frame() -> Option<Vec<u8>> {
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
pub fn decode_preamble(bytes: &[u8]) -> Option<Vec<(String, Vec<String>)>> {
    let mut dec = Decoder::new(bytes);
    let outer = dec.read_only_value().ok()?;
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
}
