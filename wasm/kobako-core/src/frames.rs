//! The Frames an invocation reads from stdin before it runs anything.
//!
//! Each frame is a 4-byte big-endian u32 length plus its bytes
//! (docs/wire/abi.md § Invocation channels). Reading and decoding them
//! here means a flow starts from the bound paths and snippets, never
//! from the channel; a frame that does not read or decode answers the
//! invocation as a boot failure before any flow runs.

use std::io::Read;

use kobako_transport::abi::{FRAME_LEN_SIZE, MAX_FRAME_LEN};
use kobako_transport::envelope::{Bindings, ErrorRecord, Frames, Origin, Outcome, Panic, Snippets};

/// `__kobako_eval`'s inputs: the Frames and the user source riding
/// between them.
pub fn read_eval_frames() -> Result<(Frames, Vec<u8>), Outcome> {
    read_eval_frames_from(&mut std::io::stdin().lock())
}

/// `__kobako_run`'s inputs, which carry no source.
pub fn read_frames() -> Result<Frames, Outcome> {
    read_frames_from(&mut std::io::stdin().lock())
}

fn read_eval_frames_from<R: Read>(input: &mut R) -> Result<(Frames, Vec<u8>), Outcome> {
    let bindings = read_bindings(input)?;
    let source = read_frame_from(input).ok_or_else(|| boot_failure("failed to read the script"))?;
    let snippets = read_snippets(input)?;
    Ok((Frames { bindings, snippets }, source))
}

fn read_frames_from<R: Read>(input: &mut R) -> Result<Frames, Outcome> {
    let bindings = read_bindings(input)?;
    let snippets = read_snippets(input)?;
    Ok(Frames { bindings, snippets })
}

fn read_bindings<R: Read>(input: &mut R) -> Result<Bindings, Outcome> {
    let bytes = read_frame_from(input)
        .ok_or_else(|| boot_failure("failed to read the Sandbox setup data"))?;
    Bindings::decode(&bytes).map_err(|_| boot_failure("failed to decode the Sandbox setup data"))
}

fn read_snippets<R: Read>(input: &mut R) -> Result<Snippets, Outcome> {
    let bytes = read_frame_from(input)
        .ok_or_else(|| boot_failure("failed to read the preloaded snippets"))?;
    Snippets::decode(&bytes).map_err(|_| boot_failure("failed to decode the preloaded snippets"))
}

/// The failure a guest reports when it cannot start the Sandbox, under
/// the class the host raises for it.
fn boot_failure(message: &str) -> Outcome {
    Outcome::Panic(Panic {
        origin: Origin::Sandbox,
        error: ErrorRecord {
            name: "Kobako::BootError".into(),
            message: message.into(),
            backtrace: Vec::new(),
        },
        available: Vec::new(),
    })
}

/// `None` on EOF, a short read, or a length past the cap, so a hostile
/// prefix never sizes an allocation.
fn read_frame_from<R: Read>(input: &mut R) -> Option<Vec<u8>> {
    let mut len_buf = [0u8; FRAME_LEN_SIZE];
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
    use kobako_transport::envelope::Snippet;

    fn framed(frames: &[&[u8]]) -> std::io::Cursor<Vec<u8>> {
        let mut bytes = Vec::new();
        for frame in frames {
            bytes.extend_from_slice(&(frame.len() as u32).to_be_bytes());
            bytes.extend_from_slice(frame);
        }
        std::io::Cursor::new(bytes)
    }

    fn boot_message(outcome: Outcome) -> String {
        let Outcome::Panic(panic) = outcome else {
            panic!("a frame that fails must answer with a Panic");
        };
        assert_eq!(
            (panic.origin, panic.error.name.as_str()),
            (Origin::Sandbox, "Kobako::BootError")
        );
        panic.error.message
    }

    // @behavior WE-066
    #[test]
    fn read_frame_from_round_trips_a_prefixed_payload() {
        let mut cursor = framed(&[b"hello"]);
        assert_eq!(read_frame_from(&mut cursor), Some(b"hello".to_vec()));
    }

    // @behavior WE-067
    #[test]
    fn read_frame_from_rejects_an_over_cap_length_prefix() {
        let mut framed = ((MAX_FRAME_LEN as u32) + 1).to_be_bytes().to_vec();
        framed.extend_from_slice(b"x");
        let mut cursor = std::io::Cursor::new(framed);
        assert_eq!(read_frame_from(&mut cursor), None);
    }

    #[test]
    fn eval_reads_the_source_between_the_two_frames() {
        // Frame 1: one path "A"; Frame 3: one source snippet "N" / "b".
        let bindings = [0, 0, 0, 1, 0, 0, 0, 1, b'A'];
        let snippets = [0, 0, 0, 1, 0, 0, 0, 0, 1, b'N', 0, 0, 0, 1, b'b'];
        let mut input = framed(&[&bindings, b"1 + 1", &snippets]);

        assert_eq!(
            read_eval_frames_from(&mut input),
            Ok((
                Frames {
                    bindings: Bindings {
                        paths: vec!["A".into()]
                    },
                    snippets: Snippets {
                        entries: vec![Snippet::Source {
                            name: "N".into(),
                            body: "b".into()
                        }]
                    },
                },
                b"1 + 1".to_vec()
            )),
            "the eval channel through read_eval_frames must yield Frame 1, the source, and \
             Frame 3 in that order"
        );
    }

    #[test]
    fn a_missing_frame_answers_as_a_boot_failure() {
        let mut input = framed(&[&[0, 0, 0, 0]]);

        assert_eq!(
            boot_message(read_frames_from(&mut input).unwrap_err()),
            "failed to read the preloaded snippets",
            "a run channel that ends before Frame 3 through read_frames must answer the \
             invocation as a boot failure"
        );
    }

    #[test]
    fn an_undecodable_frame_answers_as_a_boot_failure() {
        let mut input = framed(&[&[0xff]]);

        assert_eq!(
            boot_message(read_frames_from(&mut input).unwrap_err()),
            "failed to decode the Sandbox setup data",
            "a Frame 1 that does not decode through read_frames must answer the invocation as \
             a boot failure"
        );
    }
}
