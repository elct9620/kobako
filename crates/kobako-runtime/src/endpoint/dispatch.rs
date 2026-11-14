//! Host side of the `upcall` and `preamble` imports: hand the guest's Call
//! to the bound `DispatchHandler` and write its Reply back into guest
//! memory, or write the Invocation's Preamble there. A request under a tag
//! the host does not serve is answered as unsupported, so a guest asking
//! for a later operation can fall back.
//!
//! A failure lands on whoever is answerable for it, and always says why.
//! A trap the guest raises during a callback into it ends the invocation
//! as that trap. A Call the host could not answer is answered as an
//! internal failure naming why, and an answer too large for a message as
//! the Service's own failure. Only an answer that cannot be written into
//! guest memory at all ends the invocation, as a trap naming why.

use std::sync::Arc;

use kobako_transport::envelope::{
    Downcall, Fault, FaultKind, Reply, Unsupported, Upcall, YieldReply,
};

use super::buffer::{
    alloc_and_write, checked_payload_len, lift_list, lower_list, read_buffer, ReadError, WriteError,
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

/// Answer one `upcall` with the Reply, named in the return area at `ret`,
/// or what ends the invocation instead.
pub fn handle<G: GuestInstance>(
    guest: &mut G,
    req_ptr: i32,
    req_len: i32,
    ret: i32,
) -> Result<(), UpcallEnd<G::Fault>> {
    let reply = reply_to(guest, req_ptr, req_len)
        .unwrap_or_else(|reason| Reply::Fault(Fault::new(FaultKind::Internal, reason)));
    let written = lower_from_callback(guest, &fit_in_message(reply).encode(), ret as u32);
    if let Some(trap) = guest.calls().reentry_trap.take() {
        return Err(UpcallEnd::Fault(trap));
    }
    if guest.calls().abandoned() {
        return Err(UpcallEnd::Abandoned);
    }
    written.map_err(UpcallEnd::Unwritable)
}

/// Answer one `preamble` call with `preamble`, the Invocation's Preamble
/// bytes, named in the return area at `ret`. Each call writes the same
/// bytes afresh, so the guest may ask as often as it likes.
pub fn hand_preamble<G: GuestInstance>(
    guest: &mut G,
    preamble: &[u8],
    ret: i32,
) -> Result<(), UpcallEnd<G::Fault>> {
    let written = lower_from_callback(guest, preamble, ret as u32);
    if let Some(trap) = guest.calls().reentry_trap.take() {
        return Err(UpcallEnd::Fault(trap));
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
    // The endpoint decodes the Envelope, here and in `FrameYielder`, so
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

/// Write `bytes` into the guest from inside a callback, answering where. A
/// guest that has already trapped is not called again.
fn write_from_callback<G: GuestInstance>(guest: &mut G, bytes: &[u8]) -> Result<u32, &'static str> {
    refuse_after_trap(guest)?;
    alloc_and_write(guest, bytes).map_err(|err| callback_reason(guest, err))
}

/// Write `bytes` into the guest from inside a callback and name them in the
/// return area at `ret`, as a host import answers a list.
fn lower_from_callback<G: GuestInstance>(
    guest: &mut G,
    bytes: &[u8],
    ret: u32,
) -> Result<(), &'static str> {
    refuse_after_trap(guest)?;
    lower_list(guest, bytes, ret).map_err(|err| callback_reason(guest, err))
}

fn refuse_after_trap<G: GuestInstance>(guest: &mut G) -> Result<(), &'static str> {
    match guest.calls().reentry_trap {
        Some(_) => Err("the Sandbox already trapped during this invocation"),
        None => Ok(()),
    }
}

fn callback_reason<G: GuestInstance>(guest: &mut G, err: WriteError<G::Fault>) -> &'static str {
    match err {
        WriteError::TooLarge => "payload exceeds the 16 MiB limit",
        WriteError::Incompatible(message) => message,
        WriteError::Trapped(trap) => keep_trap(
            guest,
            trap,
            "the Sandbox trapped while allocating memory for the request",
        ),
        WriteError::OutOfBounds => "could not write the request into the Sandbox's memory",
    }
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

    let ret = guest
        .downcall(req_ptr, len_i32)
        .ok_or(RUNTIME_INCOMPATIBLE)?
        .map_err(|trap| keep_trap(guest, trap, "the Sandbox trapped while invoking a block"))?;
    let memory = guest.memory().ok_or(SANDBOX_RUNTIME_NOT_KOBAKO)?;
    let reply = lift_list(memory, ret).map_err(|err| match err {
        ReadError::TooLarge => "block result payload exceeds the 16 MiB limit",
        ReadError::OutOfBounds => "the Sandbox returned an out-of-bounds block result",
    })?;
    if reply.is_empty() {
        return Err("the Sandbox returned an empty block result");
    }
    Ok(reply)
}
