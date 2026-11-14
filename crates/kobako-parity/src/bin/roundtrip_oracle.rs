//! Round-trip oracle binary used by the Ruby host fuzz harness.
//!
//! Spawned once per fuzz test run as a long-lived subprocess; the Ruby
//! driver streams length-prefixed frames over stdin and reads the
//! re-encoded frames back from stdout. Per-iteration `cargo run` would
//! be far too slow.
//!
//! ## Frame format (Ruby <-> oracle)
//!
//! Each frame is a 4-byte big-endian length followed by `length` bytes of
//! payload.
//!
//! * Input frame (Ruby -> oracle): payload is exactly one msgpack value
//!   produced by `Kobako::Codec::Encoder`.
//! * Result frame (oracle -> Ruby):
//!     - `length` with the high bit clear: payload is the re-encoded msgpack
//!       value the oracle produced after a decode + re-encode cycle.
//!     - `length` with the high bit set (0x8000_0000): error frame. The
//!       low 31 bits give the payload length; the first payload byte is a
//!       single-character tag identifying the `codec::Error` variant
//!       (`'T'`, `'I'`, `'U'`, `'H'`, `'P'`); remaining bytes are
//!       a UTF-8 diagnostic. The Ruby side asserts no error frame is ever
//!       emitted during a clean fuzz run.
//!
//! EOF on stdin is the normal exit path (clean shutdown, status 0). Any
//! IO error is fatal (status 1).
//!
//! No deps beyond the codec under test and `std`.

#[path = "../frame.rs"]
mod frame;

use std::io;

use kobako_codec::msgpack::codec::{self, Decoder, Encoder};

fn main() {
    if let Err(e) = run() {
        eprintln!("roundtrip_oracle fatal: {e}");
        std::process::exit(1);
    }
}

fn run() -> io::Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut input = stdin.lock();
    let mut output = stdout.lock();

    while let Some(payload) = frame::read(&mut input)? {
        match roundtrip_once(&payload) {
            Ok(re_encoded) => frame::write(&mut output, &re_encoded, false)?,
            Err((tag, msg)) => {
                let mut buf = Vec::with_capacity(1 + msg.len());
                buf.push(tag);
                buf.extend_from_slice(msg.as_bytes());
                frame::write(&mut output, &buf, true)?;
            }
        }
    }
    Ok(())
}

/// Decode the payload with the guest codec, then re-encode the resulting
/// `Value` and return the new bytes. If the input is well-formed and was
/// produced by a conforming Ruby encoder, the output bytes must equal the
/// input bytes (narrowest-encoding rule).
fn roundtrip_once(input: &[u8]) -> Result<Vec<u8>, (u8, String)> {
    let mut dec = Decoder::new(input);
    let value = dec.read_value().map_err(codec_to_tag)?;
    if !dec.is_at_end() {
        return Err((b'X', format!("trailing bytes at offset {}", dec.position())));
    }
    let mut enc = Encoder::new();
    enc.write_value(&value).map_err(codec_to_tag)?;
    Ok(enc.into_bytes())
}

fn codec_to_tag(e: codec::Error) -> (u8, String) {
    let tag = match e {
        codec::Error::Truncated => b'T',
        codec::Error::InvalidType => b'I',
        codec::Error::Utf8 => b'U',
        codec::Error::InvalidHandle => b'H',
        codec::Error::PayloadTooLarge => b'P',
        // Structural faults are produced by the envelope layer, not by
        // the value-level round-trip this oracle drives; unreachable here.
        codec::Error::Malformed(_) => b'M',
        // A kind added since this mapping was written. Folding it into an
        // existing tag would let the differential agree on two refusals
        // that are not the same one, so it stops here instead.
        ref other => panic!("codec::Error kind has no differential tag: {other:?}"),
    };
    (tag, e.to_string())
}
