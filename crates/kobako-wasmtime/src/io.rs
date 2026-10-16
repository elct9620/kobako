//! Per-invocation I/O between the host and the guest: it writes the
//! entry's downcall into a freshly allocated guest buffer and reads the
//! Outcome the downcall answered with.

use wasmtime::Store as WtStore;

use crate::exports::Exports;
use crate::guest_mem::{self, ReadError, WriteError};
use crate::invocation::Invocation;
use kobako_runtime::error::{InvokeError, SetupError, Trap};
use kobako_transport::envelope::{CompileReply, Outcome};

/// A missing or trapping allocator is an engine fault, a `Trap`; one that
/// runs but returns 0 leaves the runtime intact, so it is a `SetupError`.
pub(crate) fn write_request(
    store: &mut WtStore<Invocation>,
    exports: &Exports,
    envelope: &[u8],
) -> Result<(i32, i32), InvokeError> {
    let trap = |message: String| -> InvokeError { Trap::Other(message).into() };
    let ptr =
        guest_mem::alloc_and_write(&mut *store, exports, envelope).map_err(|err| match err {
            WriteError::OutOfMemory => {
                SetupError::Intact("could not allocate input buffer (out of memory)".to_string())
                    .into()
            }
            WriteError::TooLarge => trap("payload exceeds the 16 MiB limit".to_string()),
            WriteError::Incompatible(message) => trap(message.to_string()),
            WriteError::Trapped(e) => trap(format!("failed to allocate input buffer: {e}")),
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
pub(crate) fn read_outcome(
    store: &WtStore<Invocation>,
    exports: &Exports,
    packed: u64,
) -> Result<Outcome, Trap> {
    let bytes = read_answer(store, exports, packed)?;
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
pub(crate) fn read_compile_reply(
    store: &WtStore<Invocation>,
    exports: &Exports,
    packed: u64,
) -> Result<CompileReply, Trap> {
    let bytes = read_answer(store, exports, packed)?;
    CompileReply::decode(&bytes)
        .map_err(|_| Trap::Other("Sandbox produced an unrecognised compile answer".to_string()))
}

fn read_answer(
    store: &WtStore<Invocation>,
    exports: &Exports,
    packed: u64,
) -> Result<Vec<u8>, Trap> {
    let mem = exports.require_memory()?;
    let (ptr, len) = guest_mem::unpack_guest_buffer(packed);
    guest_mem::read_buffer(store, mem, ptr, len).map_err(|err| {
        Trap::Other(
            match err {
                ReadError::TooLarge => "result payload exceeds the 16 MiB limit",
                ReadError::OutOfBounds => "the Sandbox result is out of bounds",
            }
            .to_string(),
        )
    })
}
