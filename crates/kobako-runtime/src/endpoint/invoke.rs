//! Per-invocation I/O between the host and the guest: it frames what the guest
//! is handed, writes the entry's downcall into a freshly allocated guest buffer,
//! and reads the Outcome the downcall answered with.

use kobako_transport::abi::FRAME_LEN_SIZE;
use kobako_transport::envelope::{CompileReply, Downcall, Outcome, Preamble, Run, Snippet};

use super::buffer::{
    alloc_and_write, checked_payload_len, read_buffer, unpack_guest_buffer, ReadError, WriteError,
};
use super::{GuestInstance, SANDBOX_RUNTIME_NOT_KOBAKO};
use crate::error::{InvokeError, SetupError, Trap};
use crate::runtime::Entry;

/// What the guest reads from stdin: the Preamble behind its length prefix. It
/// carries the same 16 MiB cap as a downcall request: past it, the prefix
/// would silently wrap.
pub fn stdin_content(preamble: &[u8]) -> Result<Vec<u8>, &'static str> {
    checked_payload_len(preamble.len())?;
    let mut content = Vec::with_capacity(FRAME_LEN_SIZE + preamble.len());
    content.extend_from_slice(&(preamble.len() as u32).to_be_bytes());
    content.extend_from_slice(preamble);
    Ok(content)
}

/// The downcall `entry` is.
pub fn request_of(entry: Entry<'_>) -> Vec<u8> {
    match entry {
        Entry::Eval { source } => Downcall::Eval(source.to_vec()),
        Entry::Run {
            entrypoint,
            payload,
        } => Downcall::Run(Run {
            entrypoint: entrypoint.to_owned(),
            payload,
        }),
    }
    .encode()
}

/// Write a request into a guest buffer, answering where. A missing or trapping
/// allocator is an engine fault, a `Trap`; one that runs but returns 0 leaves
/// the runtime intact, so it is a `SetupError`.
pub fn write_request<G: GuestInstance>(
    guest: &mut G,
    envelope: &[u8],
) -> Result<(i32, i32), InvokeError> {
    let trap = |message: String| -> InvokeError { Trap::Other(message).into() };
    let ptr = alloc_and_write(guest, envelope).map_err(|err| match err {
        WriteError::OutOfMemory => {
            SetupError::Intact("could not allocate input buffer (out of memory)".to_string()).into()
        }
        WriteError::TooLarge => trap("payload exceeds the 16 MiB limit".to_string()),
        WriteError::Incompatible(message) => trap(message.to_string()),
        WriteError::Trapped(e) => G::cap_trap(&e)
            .unwrap_or_else(|| Trap::Other(format!("failed to allocate input buffer: {e}")))
            .into(),
        WriteError::OutOfBounds => {
            trap("the input buffer lies outside the Sandbox's memory".to_string())
        }
    })?;
    // The write has already held the length to the payload cap.
    Ok((ptr as i32, envelope.len() as i32))
}

/// A guest that answered without an Outcome the envelope can frame — an
/// empty answer included — has nothing left to attribute, so it is a
/// `Trap` like every other way of failing to read the result.
pub fn read_outcome<G: GuestInstance>(guest: &mut G, packed: u64) -> Result<Outcome, Trap> {
    let bytes = read_answer(guest, packed)?;
    Outcome::decode(&bytes).map_err(|_| {
        Trap::Other(
            if bytes.is_empty() {
                "Sandbox exited without producing a result"
            } else {
                "Sandbox produced an unrecognised result"
            }
            .to_string(),
        )
    })
}

/// Held to the same 16 MiB bound as an Outcome; an answer that does not
/// frame is a `Trap` the caller treats like any other failed compile.
pub fn read_compile_reply<G: GuestInstance>(
    guest: &mut G,
    packed: u64,
) -> Result<CompileReply, Trap> {
    let bytes = read_answer(guest, packed)?;
    CompileReply::decode(&bytes)
        .map_err(|_| Trap::Other("Sandbox produced an unrecognised compile answer".to_string()))
}

fn read_answer<G: GuestInstance>(guest: &mut G, packed: u64) -> Result<Vec<u8>, Trap> {
    let memory = guest
        .memory()
        .ok_or_else(|| Trap::Other(SANDBOX_RUNTIME_NOT_KOBAKO.to_string()))?;
    let (ptr, len) = unpack_guest_buffer(packed);
    read_buffer(memory, ptr, len).map_err(|err| {
        Trap::Other(
            match err {
                ReadError::TooLarge => "result payload exceeds the 16 MiB limit",
                ReadError::OutOfBounds => "the Sandbox result is out of bounds",
            }
            .to_string(),
        )
    })
}

/// What a compile answered, to replay in place of `preamble`'s source
/// snippets, or `None` where nothing can be replayed.
pub fn compiled_bytecodes(preamble: &Preamble, reply: CompileReply) -> Option<Vec<Vec<u8>>> {
    match reply {
        CompileReply::Ok(bytecodes) => {
            one_per_source(preamble, bytecodes).filter(|bytecodes| replay_fits(preamble, bytecodes))
        }
        CompileReply::Error(_) | CompileReply::Unsupported(_) => None,
    }
}

/// Each invocation sends the Preamble with every bytecode in place of its
/// source, so an answer that would carry it past the payload cap would trap
/// every invocation the source alone fits.
fn replay_fits(preamble: &Preamble, bytecodes: &[Vec<u8>]) -> bool {
    let mut replay = preamble.clone();
    let sources = replay
        .snippets
        .entries
        .iter_mut()
        .filter(|entry| matches!(entry, Snippet::Source { .. }));
    for (entry, body) in sources.zip(bytecodes) {
        *entry = Snippet::Bytecode { body: body.clone() };
    }
    checked_payload_len(replay.encode().len()).is_ok()
}

/// A frontend replays each bytecode in place of one source snippet, in
/// order, so an answer of any other length leaves nothing to replay.
fn one_per_source(preamble: &Preamble, bytecodes: Vec<Vec<u8>>) -> Option<Vec<Vec<u8>>> {
    let sources = preamble
        .snippets
        .entries
        .iter()
        .filter(|entry| matches!(entry, Snippet::Source { .. }))
        .count();
    (bytecodes.len() == sources).then_some(bytecodes)
}

#[cfg(test)]
mod tests {
    use kobako_transport::envelope::Snippets;

    use super::*;

    /// One source between two bytecode snippets: a compile answers one
    /// bytecode for it.
    fn one_source() -> Preamble {
        let mut preamble = Preamble::default();
        preamble.snippets = Snippets {
            entries: vec![
                Snippet::Bytecode { body: vec![0xaa] },
                Snippet::Source {
                    name: "Only".into(),
                    body: "1".into(),
                },
                Snippet::Bytecode { body: vec![0xbb] },
            ],
        };
        preamble
    }

    // @behavior WE-089
    #[test]
    fn a_compiled_answer_of_another_length_leaves_nothing_compiled() {
        let preamble = one_source();

        assert_eq!(
            [
                one_per_source(&preamble, Vec::new()),
                one_per_source(&preamble, vec![vec![0x01], vec![0x02]]),
                one_per_source(&preamble, vec![vec![0x01]]),
            ],
            [None, None, Some(vec![vec![0x01]])],
            "a compiled answer through Driver#compile must be kept only when it carries one \
             bytecode per source snippet"
        );
    }
}
