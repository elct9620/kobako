//! Payload-codec round-trip oracle — cross-language encoder/decoder
//! agreement check between the Ruby host and the wasm guest.
//!
//! The document sibling of `roundtrip_oracle`: where that one round-trips
//! a bare value to check the type mapping, this one round-trips a whole
//! codec document to check its shape. The Ruby side sends a
//! length-prefixed frame, the oracle decodes it as a named document kind,
//! re-encodes it, and writes the bytes back; the Ruby driver asserts byte
//! equality. The two stay separate because a document carries structural
//! rules a bare value has none of, so fuzz-scale random values would fail
//! it by construction.
//!
//! ## Frame format
//!
//! The Ruby side prefixes each frame with a 1-byte document-kind tag so
//! the oracle knows which decoder to invoke:
//!
//! ```text
//! 4-byte BE length (of payload, including the kind tag)
//! 1-byte kind: a tag from `KINDS`, or '?' to list them
//! N bytes: msgpack payload for the specified document kind
//! ```
//!
//! Result frames have the same layout as `roundtrip_oracle`: a 4-byte
//! length header (high bit clear on success, set on error) followed by
//! the re-encoded bytes (no kind tag — the Ruby driver knows which kind
//! it sent). A '?' frame answers one `<tag> <type name>` line per kind,
//! so the host can hold its own payload types to the same set.
//!
//! No deps beyond the codec under test and `std`.

#[path = "../frame.rs"]
mod frame;

use std::io;

use kobako_codec::msgpack::codec;
use kobako_codec::msgpack::codec::{Decode, Encode};
use kobako_codec::msgpack::payload::Arguments;

const LIST_KINDS: u8 = b'?';

type Roundtrip = fn(&[u8]) -> Result<Vec<u8>, String>;

/// Every payload document this codec carries: its kind tag, the type name
/// its host peer shares, and the round-trip that exercises it. A payload
/// type joins the cross-language checks by joining this table.
const KINDS: &[(u8, &str, Roundtrip)] = &[(b'A', "Arguments", roundtrip_as::<Arguments>)];

fn main() {
    if let Err(e) = run() {
        eprintln!("payload_oracle fatal: {e}");
        std::process::exit(1);
    }
}

fn run() -> io::Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut input = stdin.lock();
    let mut output = stdout.lock();

    while let Some(payload) = frame::read(&mut input)? {
        // Every request leads with the kind it asks about.
        let Some((&kind, body)) = payload.split_first() else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "a request frame names no kind",
            ));
        };
        match answer(kind, body) {
            Ok(out) => frame::write(&mut output, &out, false)?,
            Err(msg) => frame::write(&mut output, msg.as_bytes(), true)?,
        }
    }
    Ok(())
}

fn answer(kind: u8, body: &[u8]) -> Result<Vec<u8>, String> {
    if kind == LIST_KINDS {
        return Ok(list_kinds());
    }
    match KINDS.iter().find(|(tag, _, _)| *tag == kind) {
        Some((_, _, roundtrip)) => roundtrip(body),
        None => Err(format!("unknown document kind {:#04x}", kind)),
    }
}

fn roundtrip_as<T: Decode + Encode>(body: &[u8]) -> Result<Vec<u8>, String> {
    T::decode(body)
        .map_err(stringify)?
        .encode()
        .map_err(stringify)
}

fn list_kinds() -> Vec<u8> {
    KINDS
        .iter()
        .map(|(tag, name, _)| format!("{} {name}", *tag as char))
        .collect::<Vec<_>>()
        .join("\n")
        .into_bytes()
}

fn stringify(e: codec::Error) -> String {
    e.to_string()
}
