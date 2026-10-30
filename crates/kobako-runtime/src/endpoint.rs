//! The host ABI endpoint: everything the kobako ABI asks of a host that no
//! Engine decides, so a Driver for any Engine implements `GuestInstance`
//! over its own memory and exports, and links `__kobako_upcall` to
//! `dispatch::handle`.
//!
//! Only this endpoint and its guest counterpart read or write an envelope's
//! bytes.

mod buffer;
mod capture;
mod dispatch;
mod invoke;
mod memory_cap;

use std::fmt::Display;

use crate::error::Trap;

pub use capture::{clip_capture, pipe_capacity};
pub use dispatch::{handle, Calls, UpcallEnd, ABANDONED};
pub use invoke::{
    compiled_bytecodes, read_compile_reply, read_outcome, request_of, stdin_content, write_request,
};
pub use memory_cap::{MemoryCap, MemoryLimitExceeded};

/// For a required guest export that is absent or mistyped, wherever it is
/// looked up. Names neither the export nor a frontend's artifact path;
/// what a caller can do is rebuild the runtime.
pub const RUNTIME_INCOMPATIBLE: &str =
    "the Sandbox runtime is incompatible; rebuild it against the kobako version in use";

/// For the "the loaded Wasm module is not a Kobako-shaped runtime at all"
/// failure mode, such as an instance with no `memory` export.
const SANDBOX_RUNTIME_NOT_KOBAKO: &str =
    "the loaded Wasm module is not a Kobako-compatible runtime";

/// One guest instance as the endpoint drives it: what a Driver gives it of an
/// Engine, from the invocation body and from inside `__kobako_upcall` alike.
/// A method answering `None` names an export the module does not have.
pub trait GuestInstance {
    /// What a call into the guest raises, kept as it is so an invocation ends
    /// with its own kind.
    type Fault: Display;

    /// The state of the Calls open on this instance.
    fn calls(&mut self) -> &mut Calls<Self::Fault>;

    /// The guest's linear memory.
    fn memory(&mut self) -> Option<&mut [u8]>;

    /// Allocate `len` bytes through `__kobako_alloc`, answering the address,
    /// or 0 where the guest has none to give.
    fn alloc(&mut self, len: u32) -> Option<Result<u32, Self::Fault>>;

    /// Call `__kobako_downcall` with a request at `ptr`, answering the packed
    /// `(ptr, len)` of its answer.
    fn downcall(&mut self, ptr: i32, len: i32) -> Option<Result<u64, Self::Fault>>;

    /// The cap `fault` carries, if one of the invocation's caps fired, which
    /// names itself rather than the step it fired during.
    fn cap_trap(fault: &Self::Fault) -> Option<Trap>;
}

#[cfg(test)]
mod tests {
    //! Witness that the endpoint needs nothing of an Engine: an instance made
    //! of a plain buffer drives a whole request and upcall through it.
    use kobako_transport::abi::unpack_ptr_len;
    use kobako_transport::envelope::Reply;

    use super::*;

    /// An instance holding a buffer and a bump allocator.
    struct InMemory {
        memory: Vec<u8>,
        next: u32,
        calls: Calls<String>,
    }

    impl GuestInstance for InMemory {
        type Fault = String;

        fn calls(&mut self) -> &mut Calls<String> {
            &mut self.calls
        }

        fn memory(&mut self) -> Option<&mut [u8]> {
            Some(&mut self.memory)
        }

        fn alloc(&mut self, len: u32) -> Option<Result<u32, String>> {
            let ptr = self.next;
            self.next += len;
            Some(Ok(ptr))
        }

        fn downcall(&mut self, _ptr: i32, _len: i32) -> Option<Result<u64, String>> {
            None
        }

        fn cap_trap(_fault: &String) -> Option<Trap> {
            None
        }
    }

    fn instance() -> InMemory {
        InMemory {
            memory: vec![0; 1024],
            next: 16,
            calls: Calls::default(),
        }
    }

    // @behavior RT-191
    #[test]
    fn a_request_lands_in_guest_memory_through_any_instance() {
        let mut guest = instance();
        let answered = write_request(&mut guest, b"request").ok();
        assert_eq!(
            (answered, &guest.memory[16..23]),
            (Some((16, 7)), &b"request"[..]),
            "endpoint::write_request through an instance of a plain buffer must write the \
             request at the address its allocator answers"
        );
    }

    // A tag the host does not serve needs no handler bound to be answered.
    // @behavior RT-191
    #[test]
    fn an_upcall_answers_through_any_instance() {
        let mut guest = instance();
        let request = b"not a call";
        guest.memory[..request.len()].copy_from_slice(request);
        let packed = handle(&mut guest, 0, request.len() as i32)
            .ok()
            .expect("the upcall must answer");
        let (ptr, len) = unpack_ptr_len(packed as u64);
        let reply = Reply::decode(&guest.memory[ptr as usize..(ptr + len) as usize]);
        assert!(
            matches!(reply, Ok(Reply::Unsupported(_))),
            "endpoint::handle through an instance of a plain buffer must write the answer to a \
             request under an unserved tag back into guest memory"
        );
    }
}
