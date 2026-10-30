//! Host side of the `__kobako_upcall` import: hand the guest's Call to
//! the bound `DispatchHandler` and write its Reply back into guest memory.
//! A request under a tag the host does not serve is answered as
//! unsupported, so a guest asking for a later operation can fall back.
//!
//! A failure lands on whoever is answerable for it, and always says why.
//! A trap the guest raises during a callback into it ends the invocation
//! as that trap. A Call the host could not answer is answered as an
//! internal failure naming why, and an answer too large for a message as
//! the Service's own failure. Only an answer that cannot be written into
//! guest memory at all ends the invocation, as a trap naming why.

use std::sync::Arc;

use kobako_transport::abi::pack_ptr_len;
use kobako_transport::envelope::{
    Downcall, Fault, FaultKind, Reply, Unsupported, Upcall, YieldReply,
};

use super::buffer::{
    alloc_and_write, checked_payload_len, read_buffer, unpack_guest_buffer, ReadError, WriteError,
};
use super::{GuestInstance, RUNTIME_INCOMPATIBLE, SANDBOX_RUNTIME_NOT_KOBAKO};
use crate::dispatch::DispatchHandler;
use crate::error::Trap;
use crate::yielder::Yielder;

/// The message of the trap ending an invocation its host gave up on.
pub const ABANDONED: &str = "the host abandoned the invocation";

/// The state of the Calls open on one instance: who answers them, how many
/// stand open, and the first trap a callback into the guest raised, as the
/// guest counts the Calls it has open.
pub struct Calls<F> {
    on_dispatch: Option<Arc<dyn DispatchHandler>>,
    open: u32,
    reentry_trap: Option<F>,
}

impl<F> Default for Calls<F> {
    fn default() -> Self {
        Self {
            on_dispatch: None,
            open: 0,
            reentry_trap: None,
        }
    }
}

impl<F> Calls<F> {
    /// Have `handler` answer the Calls on this instance.
    pub fn bind(&mut self, handler: Arc<dyn DispatchHandler>) {
        self.on_dispatch = Some(handler);
    }

    /// Whether its host gave up on the invocation, which ends at the next
    /// host call it answers.
    pub fn abandoned(&self) -> bool {
        self.on_dispatch
            .as_ref()
            .is_some_and(|handler| handler.abandoned())
    }
}

/// How an upcall ends an invocation instead of answering.
pub enum UpcallEnd<F> {
    /// The guest trapped during a callback into it.
    Fault(F),
    /// The host gave the invocation up.
    Abandoned,
    /// The answer could not be written into guest memory, naming why.
    Unwritable(&'static str),
}

/// The Yielder of one upcall's frame, built over that frame's guest, so nested
/// dispatch frames each carry their own and stack on the call stack with no
/// shared slot.
struct FrameYielder<'a, G> {
    guest: &'a mut G,
    frame: u32,
}

impl<G: GuestInstance> Yielder for FrameYielder<'_, G> {
    fn frame(&self) -> u32 {
        self.frame
    }

    fn yield_to_block(&mut self, frame: u32, args: &[u8]) -> Result<YieldReply, Trap> {
        let bytes =
            drive_yield(self.guest, frame, args).map_err(|msg| Trap::Other(msg.to_string()))?;
        YieldReply::decode(&bytes).map_err(|err| Trap::Other(err.to_string()))
    }
}

/// Answer one `__kobako_upcall` call with the packed `(ptr<<32)|len` of
/// the Reply, or what ends the invocation instead.
pub fn handle<G: GuestInstance>(
    guest: &mut G,
    req_ptr: i32,
    req_len: i32,
) -> Result<i64, UpcallEnd<G::Fault>> {
    let reply = reply_to(guest, req_ptr, req_len)
        .unwrap_or_else(|reason| Reply::Fault(Fault::new(FaultKind::Internal, reason)));
    let written = write_reply(guest, &fit_in_message(reply).encode());
    if let Some(trap) = guest.calls().reentry_trap.take() {
        return Err(UpcallEnd::Fault(trap));
    }
    if guest.calls().abandoned() {
        return Err(UpcallEnd::Abandoned);
    }
    written.map_err(UpcallEnd::Unwritable)
}

/// An answer past the message cap is the Service's to change, as any other
/// answer the host cannot write is.
fn fit_in_message(reply: Reply) -> Reply {
    let bytes = reply.encode().len();
    match checked_payload_len(bytes) {
        Ok(_) => reply,
        Err(limit) => Reply::Fault(Fault::new(
            FaultKind::Runtime,
            format!("Sandbox could not write the Service's answer: {limit}"),
        )),
    }
}

fn reply_to<G: GuestInstance>(
    guest: &mut G,
    req_ptr: i32,
    req_len: i32,
) -> Result<Reply, &'static str> {
    let req_bytes = read_request(guest, req_ptr, req_len)?;
    // The endpoint decodes the core envelope, here and in `FrameYielder`, so
    // the frontend never sees a frame; the payload inside it stays bytes
    // the whole way through.
    let call = match Upcall::decode(&req_bytes) {
        Ok(Upcall::Call(call)) => call,
        Ok(_) => {
            return Ok(Reply::Unsupported(Unsupported::new(
                "the host does not serve this request",
            )))
        }
        Err(_) => {
            return Err(
                "the guest sent a malformed Call envelope — please report this as a kobako bug",
            )
        }
    };

    // A frontend always installs the dispatch handler before invoking the
    // runtime, so reaching this branch indicates a misuse rather than a
    // normal control path.
    let handler = guest
        .calls()
        .on_dispatch
        .clone()
        .ok_or("a Sandbox callback fired outside an active invocation — please report this as a kobako bug")?;

    // A frame-scoped yielder lends the guest to the handler. The borrow ends
    // with the block, freeing it for `write_reply`; nested dispatch frames
    // each build their own, so the LIFO re-entry lives on the stack.
    let frame = {
        let calls = guest.calls();
        calls.open += 1;
        calls.open - 1
    };
    let reply = {
        let mut yielder = FrameYielder { guest, frame };
        handler.dispatch(call, &mut yielder)
    };
    guest.calls().open -= 1;
    reply.ok_or(
        "a Sandbox callback raised an exception instead of returning a fault — please report this as a kobako bug",
    )
}

fn write_reply<G: GuestInstance>(guest: &mut G, bytes: &[u8]) -> Result<i64, &'static str> {
    let ptr = write_from_callback(guest, bytes)?;
    // The write has already held the length to the payload cap.
    Ok(pack_ptr_len(ptr, bytes.len() as u32) as i64)
}

/// The request the guest hands the upcall import. Its `(ptr, len)` are
/// guest addresses, so they are read unsigned.
fn read_request<G: GuestInstance>(
    guest: &mut G,
    req_ptr: i32,
    req_len: i32,
) -> Result<Vec<u8>, &'static str> {
    let memory = guest.memory().ok_or(SANDBOX_RUNTIME_NOT_KOBAKO)?;
    read_buffer(memory, req_ptr as u32 as usize, req_len as u32 as usize).map_err(|err| match err {
        ReadError::TooLarge => "request payload exceeds the 16 MiB limit",
        ReadError::OutOfBounds => "the Sandbox produced an out-of-bounds request",
    })
}

/// Keep a trap the guest raised during a callback into it for the upcall to
/// end the invocation with, and answer the callback's own reason.
fn keep_trap<G: GuestInstance>(
    guest: &mut G,
    trap: G::Fault,
    reason: &'static str,
) -> &'static str {
    guest.calls().reentry_trap.get_or_insert(trap);
    reason
}

/// Write `bytes` into the guest from inside a callback. A guest that has
/// already trapped is not called again.
fn write_from_callback<G: GuestInstance>(guest: &mut G, bytes: &[u8]) -> Result<u32, &'static str> {
    if guest.calls().reentry_trap.is_some() {
        return Err("the Sandbox already trapped during this invocation");
    }
    alloc_and_write(guest, bytes).map_err(|err| match err {
        WriteError::TooLarge => "payload exceeds the 16 MiB limit",
        WriteError::Incompatible(message) => message,
        WriteError::Trapped(trap) => keep_trap(
            guest,
            trap,
            "the Sandbox trapped while allocating memory for the request",
        ),
        WriteError::OutOfMemory => "the Sandbox ran out of memory while preparing the request",
        WriteError::OutOfBounds => "could not write the request into the Sandbox's memory",
    })
}

fn drive_yield<G: GuestInstance>(
    guest: &mut G,
    frame: u32,
    args: &[u8],
) -> Result<Vec<u8>, &'static str> {
    let request = Downcall::Yield {
        frame,
        args: args.to_vec(),
    }
    .encode();
    let len_i32 = checked_payload_len(request.len())?;
    let req_ptr = write_from_callback(guest, &request)? as i32;

    let packed = guest
        .downcall(req_ptr, len_i32)
        .ok_or(RUNTIME_INCOMPATIBLE)?
        .map_err(|trap| keep_trap(guest, trap, "the Sandbox trapped while invoking a block"))?;
    let (reply_ptr, reply_len) = unpack_guest_buffer(packed);
    if reply_len == 0 {
        return Err("the Sandbox returned an empty block result");
    }
    let memory = guest.memory().ok_or(SANDBOX_RUNTIME_NOT_KOBAKO)?;
    read_buffer(memory, reply_ptr, reply_len).map_err(|err| match err {
        ReadError::TooLarge => "block result payload exceeds the 16 MiB limit",
        ReadError::OutOfBounds => "the Sandbox returned an out-of-bounds block result",
    })
}
