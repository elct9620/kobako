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
//! 1-byte kind: 'A' invocation Arguments
//! N bytes: msgpack payload for the specified document kind
//! ```
//!
//! Result frames have the same layout as `roundtrip_oracle`: a 4-byte
//! length header (high bit clear on success, set on error) followed by
//! the re-encoded bytes (no kind tag — the Ruby driver knows which kind
//! it sent).
//!
//! No deps beyond the codec under test and `std`.

use std::io::{self, Read, Write};

use kobako_codec::codec;
use kobako_codec::codec::{Decode, Encode};
use kobako_codec::payload::Arguments;
use kobako_codec::{FRAME_LEN_SIZE, MAX_FRAME_LEN};

const ERROR_FLAG: u32 = 0x8000_0000;

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

    loop {
        let mut hdr = [0u8; FRAME_LEN_SIZE];
        match input.read_exact(&mut hdr) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(e) => return Err(e),
        }
        let len = u32::from_be_bytes(hdr) as usize;
        if len == 0 || len > MAX_FRAME_LEN {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("frame length {len} out of range"),
            ));
        }
        let mut payload = vec![0u8; len];
        input.read_exact(&mut payload)?;

        let kind = payload[0];
        let body = &payload[1..];
        match roundtrip(kind, body) {
            Ok(out) => write_frame(&mut output, &out, false)?,
            Err(msg) => write_frame(&mut output, msg.as_bytes(), true)?,
        }
        output.flush()?;
    }
}

fn write_frame<W: Write>(out: &mut W, payload: &[u8], is_error: bool) -> io::Result<()> {
    let len = payload.len() as u32;
    let header = if is_error { len | ERROR_FLAG } else { len };
    out.write_all(&header.to_be_bytes())?;
    out.write_all(payload)?;
    Ok(())
}

fn roundtrip(kind: u8, body: &[u8]) -> Result<Vec<u8>, String> {
    match kind {
        b'A' => {
            let arguments = Arguments::decode(body).map_err(stringify)?;
            arguments.encode().map_err(stringify)
        }
        other => Err(format!("unknown document kind {:#04x}", other)),
    }
}

fn stringify(e: codec::Error) -> String {
    e.to_string()
}
