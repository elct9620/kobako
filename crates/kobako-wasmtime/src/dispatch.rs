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

use wasmtime::Caller;

use kobako_runtime::error::Trap;
use kobako_runtime::yielder::Yielder;
use kobako_transport::abi::pack_ptr_len;
use kobako_transport::envelope::{Fault, FaultKind, Reply, Unsupported, Upcall, YieldReply};

use crate::guest_mem;
use crate::invocation::Invocation;

/// The wasmtime-backed `Yielder`: built per `__kobako_upcall` frame over
/// that frame's `Caller`, so nested dispatch frames each carry their own and
/// stack on the Rust call stack with no shared slot.
struct CallerYielder<'a, 'c> {
    caller: &'a mut Caller<'c, Invocation>,
    frame: u32,
}

impl Yielder for CallerYielder<'_, '_> {
    fn frame(&self) -> u32 {
        self.frame
    }

    fn yield_to_block(&mut self, frame: u32, args: &[u8]) -> Result<YieldReply, Trap> {
        let bytes = guest_mem::drive_yield(self.caller, frame, args)
            .map_err(|msg| Trap::Other(msg.to_string()))?;
        YieldReply::decode(&bytes).map_err(|err| Trap::Other(err.to_string()))
    }
}

/// Answer one `__kobako_upcall` call with the packed `(ptr<<32)|len` of
/// the Reply, or the trap a callback into the guest raised.
pub(crate) fn handle(
    caller: &mut Caller<'_, Invocation>,
    req_ptr: i32,
    req_len: i32,
) -> wasmtime::Result<i64> {
    let reply = reply_to(caller, req_ptr, req_len)
        .unwrap_or_else(|reason| Reply::Fault(Fault::new(FaultKind::Internal, reason)));
    let written = write_reply(caller, &fit_in_message(reply).encode());
    if let Some(trap) = caller.data_mut().take_reentry_trap() {
        return Err(trap);
    }
    if let Some(trap) = caller.data().abandonment() {
        return Err(trap);
    }
    written.map_err(wasmtime::Error::msg)
}

/// An answer past the message cap is the Service's to change, as any other
/// answer the host cannot write is.
fn fit_in_message(reply: Reply) -> Reply {
    let bytes = reply.encode().len();
    match guest_mem::checked_payload_len(bytes) {
        Ok(_) => reply,
        Err(limit) => Reply::Fault(Fault::new(
            FaultKind::Runtime,
            format!("Sandbox could not write the Service's answer: {limit}"),
        )),
    }
}

fn reply_to(
    caller: &mut Caller<'_, Invocation>,
    req_ptr: i32,
    req_len: i32,
) -> Result<Reply, &'static str> {
    let req_bytes = guest_mem::read_request(caller, req_ptr, req_len)?;
    // The driver decodes the core envelope, here and in `CallerYielder`, so
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

    // `Kobako::Sandbox` always installs the dispatch handler before
    // invoking the runtime, so reaching this branch indicates a misuse
    // rather than a normal control path.
    let handler = caller
        .data()
        .on_dispatch()
        .ok_or("a Sandbox callback fired outside an active invocation — please report this as a kobako bug")?;

    // Build a frame-scoped yielder over this Caller and hand it to the
    // handler. The borrow ends with the block, freeing the Caller for
    // `write_reply`; nested dispatch frames each build their own, so
    // the LIFO re-entry lives on the Rust stack — no shared slot.
    let frame = caller.data_mut().open_call();
    let reply = {
        let mut yielder = CallerYielder { caller, frame };
        handler.dispatch(call, &mut yielder)
    };
    caller.data_mut().close_call();
    reply.ok_or(
        "a Sandbox callback raised an exception instead of returning a fault — please report this as a kobako bug",
    )
}

fn write_reply(caller: &mut Caller<'_, Invocation>, bytes: &[u8]) -> Result<i64, &'static str> {
    let ptr = guest_mem::write_from_callback(caller, bytes)?;
    // The write has already held the length to the payload cap.
    Ok(pack_ptr_len(ptr, bytes.len() as u32) as i64)
}
