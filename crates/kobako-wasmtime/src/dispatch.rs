//! Host side of the `__kobako_upcall` import: hand the guest's Call to
//! the bound `DispatchHandler` and write its Reply back into guest memory.
//! A request under a tag the host does not serve is answered as
//! unsupported, so a guest asking for a later operation can fall back.
//!
//! A failure lands on whoever is answerable for it. A trap the guest
//! raises during a callback into it ends the invocation as that trap; any
//! other failure is the guest's or the wire's, so the import answers 0,
//! which the guest receives as a wire failure.
//!
//! Each 0 also writes one `[kobako-dispatch] <reason>` line to stderr,
//! since the 0 itself carries no reason. It is the one place the driver
//! logs, and normal operation never reaches it.

use wasmtime::Caller;

use kobako_runtime::error::Trap;
use kobako_runtime::yielder::Yielder;
use kobako_transport::abi::pack_ptr_len;
use kobako_transport::envelope::{Unsupported, Upcall, YieldReply};

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

/// Answer one `__kobako_upcall` call: the packed `(ptr<<32)|len` of the
/// Reply, 0 for a failure the guest or the wire is answerable for, or the
/// trap a callback into the guest raised.
pub(crate) fn handle(
    caller: &mut Caller<'_, Invocation>,
    req_ptr: i32,
    req_len: i32,
) -> wasmtime::Result<i64> {
    let answer = try_handle(caller, req_ptr, req_len);
    if let Some(trap) = caller.data_mut().take_reentry_trap() {
        return Err(trap);
    }
    if let Some(trap) = caller.data().abandonment() {
        return Err(trap);
    }
    Ok(answer.unwrap_or_else(|reason| {
        eprintln!("[kobako-dispatch] {reason}");
        0
    }))
}

fn try_handle(
    caller: &mut Caller<'_, Invocation>,
    req_ptr: i32,
    req_len: i32,
) -> Result<i64, &'static str> {
    let req_bytes = guest_mem::read_request(caller, req_ptr, req_len)?;
    // The driver decodes the core envelope, here and in `CallerYielder`, so
    // the frontend never sees a frame; the payload inside it stays bytes
    // the whole way through.
    let call = match Upcall::decode(&req_bytes) {
        Ok(Upcall::Call(call)) => call,
        Ok(_) => {
            let refusal = Unsupported::new("the host does not serve this request");
            return write_reply(caller, &refusal.encode());
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
    let reply = reply.ok_or(
        "a Sandbox callback raised an exception instead of returning a fault — please report this as a kobako bug",
    )?;

    write_reply(caller, &reply.encode())
}

fn write_reply(caller: &mut Caller<'_, Invocation>, bytes: &[u8]) -> Result<i64, &'static str> {
    let ptr = guest_mem::write_from_callback(caller, bytes)?;
    // The write has already held the length to the payload cap.
    Ok(pack_ptr_len(ptr, bytes.len() as u32) as i64)
}
