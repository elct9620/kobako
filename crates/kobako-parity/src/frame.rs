//! The frame the parity harness and its oracles exchange over a pipe: a
//! big-endian length word, its high bit set for a harness-level error
//! message, then that many bytes.

use std::io::{self, Read, Write};

use kobako_transport::abi::{FRAME_LEN_SIZE, MAX_FRAME_LEN};

/// High bit of the length word: the payload is an error message, not an
/// answer.
const ERROR_FLAG: u32 = 0x8000_0000;

/// The next frame's bytes, or `None` once the peer closed the pipe between
/// frames. A frame longer than the ABI's `MAX_FRAME_LEN` is refused.
pub fn read(input: &mut impl Read) -> io::Result<Option<Vec<u8>>> {
    let mut header = [0u8; FRAME_LEN_SIZE];
    match input.read_exact(&mut header) {
        Ok(()) => {}
        Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(err) => return Err(err),
    }
    let len = u32::from_be_bytes(header) as usize;
    if len > MAX_FRAME_LEN {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("frame length {len} exceeds {MAX_FRAME_LEN}"),
        ));
    }
    let mut payload = vec![0u8; len];
    input.read_exact(&mut payload)?;
    Ok(Some(payload))
}

/// Write `payload` as one frame, marked as an error message when
/// `is_error`, and flush it so the peer reads it at once.
pub fn write(output: &mut impl Write, payload: &[u8], is_error: bool) -> io::Result<()> {
    let len = payload.len() as u32;
    let word = if is_error { len | ERROR_FLAG } else { len };
    output.write_all(&word.to_be_bytes())?;
    output.write_all(payload)?;
    output.flush()
}
