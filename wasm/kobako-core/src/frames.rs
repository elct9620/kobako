//! Stdin invocation-channel frame mechanics.
//!
//! Every invocation entry point consumes length-prefixed stdin frames
//! (4-byte big-endian u32 length + payload — docs/wire-codec.md
//! § Invocation channels): `read_frame` is the channel reader and
//! `decode_preamble` parses Frame 1 (the flat list of bind paths every
//! dispatching guest installs proxies from).
//! Frame payload semantics that belong to a guest language — e.g. the
//! bundled guest's Frame 3 snippet kinds (mruby source / RITE
//! bytecode) — stay in the implementation crate; this module carries
//! only the language-neutral wire shapes.

use kobako_codec::codec::{Decoder, Value};
use kobako_codec::MAX_FRAME_LEN;

/// Read one length-prefixed stdin frame. Returns `None` on EOF, short
/// read, or an over-cap length prefix; callers turn that into a Panic
/// envelope.
pub fn read_frame() -> Option<Vec<u8>> {
    read_frame_from(&mut std::io::stdin().lock())
}

/// Channel reader over any byte source — host-buildable so the length
/// framing and the allocation guard can be unit-tested off-target.
fn read_frame_from<R: std::io::Read>(input: &mut R) -> Option<Vec<u8>> {
    let mut len_buf = [0u8; kobako_codec::FRAME_LEN_SIZE];
    input.read_exact(&mut len_buf).ok()?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > MAX_FRAME_LEN {
        return None;
    }
    let mut payload = vec![0u8; len];
    input.read_exact(&mut payload).ok()?;
    Some(payload)
}

/// Decode the Frame 1 preamble: a flat list of bind paths
/// (`["MyService::KV", "File", ...]`). Each entry is the constant path a
/// bound Service is installed at. Pure parser — host-buildable so the
/// decoder can be unit-tested outside the wasm target.
pub fn decode_preamble(bytes: &[u8]) -> Option<Vec<String>> {
    let mut dec = Decoder::new(bytes);
    let outer = dec.read_only_value().ok()?;
    let Value::Array(items) = outer else {
        return None;
    };
    let mut paths = Vec::with_capacity(items.len());
    for item in items {
        match item {
            Value::Str(s) => paths.push(s),
            _ => return None,
        }
    }
    Some(paths)
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
    fn decode_preamble_accepts_a_flat_path_list() {
        let bytes = encode(&Value::Array(vec![
            Value::Str("KV::Get".into()),
            Value::Str("KV::Set".into()),
            Value::Str("File".into()),
        ]));
        let out = decode_preamble(&bytes).unwrap();
        assert_eq!(out, vec!["KV::Get", "KV::Set", "File"]);
    }

    #[test]
    fn decode_preamble_rejects_non_array_outer() {
        let bytes = encode(&Value::Map(Vec::new()));
        assert!(decode_preamble(&bytes).is_none());
    }

    #[test]
    fn decode_preamble_rejects_a_non_string_entry() {
        let bytes = encode(&Value::Array(vec![Value::Int(1)]));
        assert!(decode_preamble(&bytes).is_none());
    }

    #[test]
    fn read_frame_from_round_trips_a_prefixed_payload() {
        let payload = b"hello".to_vec();
        let mut framed = (payload.len() as u32).to_be_bytes().to_vec();
        framed.extend_from_slice(&payload);
        let mut cursor = std::io::Cursor::new(framed);
        assert_eq!(read_frame_from(&mut cursor), Some(payload));
    }

    #[test]
    fn read_frame_from_rejects_an_over_cap_length_prefix() {
        let mut framed = ((MAX_FRAME_LEN as u32) + 1).to_be_bytes().to_vec();
        framed.extend_from_slice(b"x");
        let mut cursor = std::io::Cursor::new(framed);
        assert_eq!(read_frame_from(&mut cursor), None);
    }
}
