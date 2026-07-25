//! Stdin invocation-channel frame mechanics.
//!
//! Every invocation entry point consumes length-prefixed stdin frames
//! (4-byte big-endian u32 length + payload — docs/wire-codec.md
//! § Invocation channels). This module carries only the channel itself;
//! what a frame's bytes mean is the core envelope's business
//! (`kobako_codec::envelope`).

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

#[cfg(test)]
mod tests {
    use super::*;

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
