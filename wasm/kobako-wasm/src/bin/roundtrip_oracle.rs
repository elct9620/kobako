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
//! * Request frame (Ruby -> oracle): payload is exactly one msgpack value
//!   produced by `Kobako::Codec::Encoder`.
//! * Response frame (oracle -> Ruby):
//!     - `length` with the high bit clear: payload is the re-encoded msgpack
//!       value the oracle produced after a decode + re-encode cycle.
//!     - `length` with the high bit set (0x8000_0000): error frame. The
//!       low 31 bits give the payload length; the first payload byte is a
//!       single-character tag identifying the `CodecError` variant
//!       (`'T'`, `'I'`, `'U'`, `'H'`, `'E'`, `'P'`); remaining bytes are
//!       a UTF-8 diagnostic. The Ruby side asserts no error frame is ever
//!       emitted during a clean fuzz run.
//!
//! EOF on stdin is the normal exit path (clean shutdown, status 0). Any
//! IO error is fatal (status 1).
//!
//! No third-party deps — only the kobako-wasm codec and `std`.

use std::io::{self, Read, Write};

use kobako_wasm::codec::{CodecError, Decoder, Encoder};
use kobako_wasm::FRAME_LEN_SIZE;

const MAX_FRAME: usize = 64 * 1024 * 1024; // 64 MiB hard cap (well above SPEC's 16 MiB single-RPC limit)
const ERROR_FLAG: u32 = 0x8000_0000;

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

    loop {
        let mut hdr = [0u8; FRAME_LEN_SIZE];
        match input.read_exact(&mut hdr) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(e) => return Err(e),
        }
        let len = u32::from_be_bytes(hdr) as usize;
        if len > MAX_FRAME {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("frame length {len} exceeds {MAX_FRAME}"),
            ));
        }
        let mut payload = vec![0u8; len];
        input.read_exact(&mut payload)?;

        match roundtrip_once(&payload) {
            Ok(re_encoded) => write_frame(&mut output, &re_encoded, false)?,
            Err((tag, msg)) => {
                let mut buf = Vec::with_capacity(1 + msg.len());
                buf.push(tag);
                buf.extend_from_slice(msg.as_bytes());
                write_frame(&mut output, &buf, true)?;
            }
        }
        output.flush()?;
    }
}

fn write_frame<W: Write>(out: &mut W, payload: &[u8], is_error: bool) -> io::Result<()> {
    let len = payload.len() as u32;
    let header_word = if is_error { len | ERROR_FLAG } else { len };
    out.write_all(&header_word.to_be_bytes())?;
    out.write_all(payload)?;
    Ok(())
}

/// Decode the payload with the guest codec, then re-encode the resulting
/// `Value` and return the new bytes. If the input is well-formed and was
/// produced by SPEC-compliant Ruby encoder, the output bytes must equal the
/// input bytes (narrowest-encoding rule).
fn roundtrip_once(input: &[u8]) -> Result<Vec<u8>, (u8, String)> {
    let mut dec = Decoder::new(input);
    let value = dec.read_value().map_err(wire_to_tag)?;
    if !dec.at_end() {
        return Err((b'X', format!("trailing bytes at offset {}", dec.position())));
    }
    let mut enc = Encoder::with_capacity(input.len());
    enc.write_value(&value).map_err(wire_to_tag)?;
    Ok(enc.into_bytes())
}

fn wire_to_tag(e: CodecError) -> (u8, String) {
    let tag = match e {
        CodecError::Truncated => b'T',
        CodecError::InvalidType => b'I',
        CodecError::Utf8 => b'U',
        CodecError::InvalidHandle => b'H',
        CodecError::InvalidErrEnv => b'E',
        CodecError::PayloadTooLarge => b'P',
    };
    (tag, e.to_string())
}
