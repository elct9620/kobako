//! Envelope-layer round-trip oracle — cross-side encoder/decoder
//! agreement check between the Ruby host and the wasm guest.
//!
//! This is the envelope sibling of `roundtrip_oracle`: the Ruby side sends
//! a length-prefixed frame, the oracle decodes it as a specific envelope
//! kind, re-encodes it, and writes the bytes back. The Ruby driver then
//! asserts byte equality (proving the two SPEC implementations agree on
//! the envelope-level framing, not just the underlying msgpack codec).
//!
//! ## Frame format
//!
//! The Ruby side prefixes each frame with a 1-byte envelope-kind tag so
//! the oracle knows which decoder to invoke:
//!
//! ```text
//! 4-byte BE length (of payload, including the kind tag)
//! 1-byte kind: 'Q' Request, 'P' Response, 'R' Result envelope,
//!              'X' Panic envelope, 'O' Outcome envelope
//! N bytes: msgpack payload for the specified envelope kind
//! ```
//!
//! Response frames have the same layout as `roundtrip_oracle`: a 4-byte
//! length header (high bit clear on success, set on error) followed by
//! the re-encoded bytes (no kind tag — the Ruby driver knows which kind
//! it sent).
//!
//! No third-party deps.

use std::io::{self, Read, Write};

use kobako_wasm::outcome::{
    decode_outcome, decode_panic, decode_result, encode_outcome, encode_panic, encode_result,
};
use kobako_wasm::rpc::envelope::{
    decode_request, decode_response, encode_request, encode_response, EnvelopeError,
};
use kobako_wasm::FRAME_LEN_SIZE;

const MAX_FRAME: usize = 64 * 1024 * 1024;
const ERROR_FLAG: u32 = 0x8000_0000;

fn main() {
    if let Err(e) = run() {
        eprintln!("envelope_oracle fatal: {e}");
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
        if len == 0 || len > MAX_FRAME {
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
        b'Q' => {
            let req = decode_request(body).map_err(stringify)?;
            encode_request(&req).map_err(stringify)
        }
        b'P' => {
            let resp = decode_response(body).map_err(stringify)?;
            encode_response(&resp).map_err(stringify)
        }
        b'R' => {
            let v = decode_result(body).map_err(stringify)?;
            encode_result(&v).map_err(stringify)
        }
        b'X' => {
            let p = decode_panic(body).map_err(stringify)?;
            encode_panic(&p).map_err(stringify)
        }
        b'O' => {
            let o = decode_outcome(body).map_err(stringify)?;
            encode_outcome(&o).map_err(stringify)
        }
        other => Err(format!("unknown envelope kind {:#04x}", other)),
    }
}

fn stringify(e: EnvelopeError) -> String {
    e.to_string()
}
