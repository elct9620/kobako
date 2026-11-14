//! Per-invocation I/O between the host and the guest: it encodes the Preamble
//! the guest takes, writes the entry's downcall into room the guest gives, and
//! reads the Outcome the downcall answered with.

use kobako_transport::envelope::{CompileReply, Downcall, Outcome, Preamble, Run, Snippet};

use super::buffer::{alloc_and_write, checked_payload_len, lift_list, ReadError, WriteError};
use super::{GuestInstance, SANDBOX_RUNTIME_NOT_KOBAKO};
use crate::error::{InvokeError, Trap};
use crate::runtime::Entry;

/// The bytes the host hands the guest through its `preamble` import, held to
/// the 16 MiB cap every message carries, so an Invocation whose Preamble is
/// past it fails before anything runs.
pub fn preamble_bytes(preamble: &Preamble) -> Result<Vec<u8>, &'static str> {
    let bytes = preamble.encode();
    checked_payload_len(bytes.len())?;
    Ok(bytes)
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

/// The downcall that asks the guest to compile its Preamble.
pub fn compile_request() -> Vec<u8> {
    Downcall::Compile.encode()
}

/// Write a request into room the guest gives, answering where. A missing
/// allocator, or one that traps for want of room, is a `Trap`.
pub fn write_request<G: GuestInstance>(
    guest: &mut G,
    envelope: &[u8],
) -> Result<(i32, i32), InvokeError> {
    let trap = |message: String| -> InvokeError { Trap::Other(message).into() };
    let ptr = alloc_and_write(guest, envelope).map_err(|err| match err {
        WriteError::TooLarge => trap("payload exceeds the 16 MiB limit".to_string()),
        WriteError::Incompatible(message) => trap(message.to_string()),
        WriteError::Trapped(e) => G::cap_trap_of(&e)
            .unwrap_or_else(|| Trap::Other(format!("could not allocate input buffer: {e}")))
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
pub fn read_outcome<G: GuestInstance>(guest: &mut G, ret: u32) -> Result<Outcome, Trap> {
    let bytes = read_answer(guest, ret)?;
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
pub fn read_compile_reply<G: GuestInstance>(guest: &mut G, ret: u32) -> Result<CompileReply, Trap> {
    let bytes = read_answer(guest, ret)?;
    CompileReply::decode(&bytes)
        .map_err(|_| Trap::Other("Sandbox produced an unrecognised compile answer".to_string()))
}

/// The answer the return area at `ret` names.
fn read_answer<G: GuestInstance>(guest: &mut G, ret: u32) -> Result<Vec<u8>, Trap> {
    let memory = guest
        .memory()
        .ok_or_else(|| Trap::Other(SANDBOX_RUNTIME_NOT_KOBAKO.to_string()))?;
    lift_list(memory, ret).map_err(|err| {
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
