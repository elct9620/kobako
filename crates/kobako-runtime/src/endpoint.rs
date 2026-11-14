//! The host ABI endpoint: everything the kobako ABI asks of a host that no
//! Engine decides, so a Wasm Driver for any Engine implements `GuestInstance`
//! over its own memory and exports, and links the `upcall` and `preamble`
//! imports to `handle` and `hand_preamble`.
//!
//! Only this endpoint and its guest counterpart read or write an envelope's
//! bytes.

mod buffer;
mod dispatch;
mod invoke;

use std::fmt::Display;

use kobako_transport::abi::{ABI_VERSION, DOWNCALL_EXPORT, GUEST_INTERFACE};

use crate::error::{SetupError, Trap};

pub use dispatch::{hand_preamble, handle, Calls, ImportEnd, ABANDONED};
pub use invoke::{
    compile_request, compiled_bytecodes, preamble_bytes, read_compile_reply, read_outcome,
    request_of, write_request,
};

/// For a required guest export that is absent or mistyped, wherever it is
/// looked up. Names neither the export nor a frontend's artifact path;
/// what a caller can do is rebuild the runtime.
pub const RUNTIME_INCOMPATIBLE: &str =
    "the Sandbox runtime is incompatible; rebuild it against the kobako version in use";

/// Accept a Guest Binary only when, among the names of its `exports`, its
/// downcall stands under the host's ABI version. Only rebuilding the artifact
/// fixes either refusal, so both are `Dead` and say so.
pub fn verify_abi_version<'a>(
    exports: impl IntoIterator<Item = &'a str>,
) -> Result<(), SetupError> {
    let interface = GUEST_INTERFACE
        .strip_suffix(ABI_VERSION)
        .expect("the guest interface carries the ABI version");
    let downcall = DOWNCALL_EXPORT
        .strip_prefix(GUEST_INTERFACE)
        .expect("the downcall export stands under the guest interface");
    let reported = exports.into_iter().find_map(|name| {
        name.strip_prefix(interface)
            .and_then(|rest| rest.strip_suffix(downcall))
    });
    match reported {
        None => Err(SetupError::Dead(format!(
            "the Guest Binary exports no {DOWNCALL_EXPORT}; \
             rebuild it against ABI version {ABI_VERSION}"
        ))),
        Some(version) if version != ABI_VERSION => Err(SetupError::Dead(format!(
            "the Guest Binary reports ABI version {version}, but this host \
             implements ABI version {ABI_VERSION}; rebuild the Guest Binary \
             against the host's version"
        ))),
        Some(_) => Ok(()),
    }
}

/// For the "the loaded Wasm module is not a Kobako-shaped runtime at all"
/// failure mode, such as an instance with no `memory` export.
const SANDBOX_RUNTIME_NOT_KOBAKO: &str =
    "the loaded Wasm module is not a Kobako-compatible runtime";

/// One guest instance as the endpoint drives it: what a Wasm Driver gives it
/// of an Engine, from the invocation body and from inside a host import alike.
/// A method answering `None` names an export the module does not have.
pub trait GuestInstance {
    /// What a call into the guest raises, kept as it is so an invocation ends
    /// with its own kind.
    type Fault: Display;

    /// The state of the Calls open on this instance.
    fn calls(&mut self) -> &mut Calls<Self::Fault>;

    /// The guest's linear memory.
    fn memory(&mut self) -> Option<&mut [u8]>;

    /// Room for `len` bytes from `cabi_realloc`, answering its address. A
    /// guest with no room left traps.
    fn realloc(&mut self, len: u32) -> Option<Result<u32, Self::Fault>>;

    /// Call the downcall with a request at `ptr`, answering the return area
    /// that names its answer.
    fn downcall(&mut self, ptr: i32, len: i32) -> Option<Result<u32, Self::Fault>>;

    /// The cap `fault` carries, if one of the invocation's caps fired, which
    /// names itself rather than the step it fired during.
    fn cap_trap_of(fault: &Self::Fault) -> Option<Trap>;
}

#[cfg(test)]
mod tests {
    //! Witness that the endpoint needs nothing of an Engine: an instance made
    //! of a plain buffer drives a whole request and upcall through it.
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

        fn realloc(&mut self, len: u32) -> Option<Result<u32, String>> {
            let ptr = self.next;
            self.next += len;
            Some(Ok(ptr))
        }

        fn downcall(&mut self, _ptr: i32, _len: i32) -> Option<Result<u32, String>> {
            None
        }

        fn cap_trap_of(_fault: &String) -> Option<Trap> {
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

    /// The list the return area at `ret` names in `guest`.
    fn named_by(guest: &InMemory, ret: usize) -> &[u8] {
        let word = |at: usize| u32::from_le_bytes(guest.memory[at..at + 4].try_into().unwrap());
        let (ptr, len) = (word(ret) as usize, word(ret + 4) as usize);
        &guest.memory[ptr..ptr + len]
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
        guest.memory[512..512 + request.len()].copy_from_slice(request);
        assert!(
            handle(&mut guest, 512, request.len() as i32, 8).is_ok(),
            "the upcall must answer"
        );
        let reply = Reply::decode(named_by(&guest, 8));
        assert!(
            matches!(reply, Ok(Reply::Unsupported(_))),
            "endpoint::handle through an instance of a plain buffer must write the answer to a \
             request under an unserved tag back into guest memory"
        );
    }

    // @behavior RT-396
    #[test]
    fn the_preamble_is_the_same_however_often_the_guest_asks() {
        let mut guest = instance();
        let answers: Vec<Vec<u8>> = (0..2)
            .map(|_| {
                assert!(hand_preamble(&mut guest, b"setup", 8).is_ok());
                named_by(&guest, 8).to_vec()
            })
            .collect();
        assert_eq!(
            answers,
            [b"setup".to_vec(), b"setup".to_vec()],
            "endpoint::hand_preamble asked twice within one Invocation must name the same \
             Preamble bytes each time"
        );
    }

    /// A host that gave the invocation up and answers no Call.
    struct GaveUp;

    impl crate::dispatch::DispatchHandler for GaveUp {
        fn dispatch(
            &self,
            _call: kobako_transport::envelope::Call<'_>,
            _yielder: &mut dyn crate::yielder::Yielder,
        ) -> Option<Reply> {
            None
        }

        fn abandoned(&self) -> bool {
            true
        }
    }

    // @behavior RT-398
    #[test]
    fn the_preamble_asked_for_after_the_host_gave_up_ends_the_invocation() {
        let mut guest = instance();
        guest.calls.bind(std::sync::Arc::new(GaveUp));
        assert!(
            matches!(
                hand_preamble(&mut guest, b"setup", 8),
                Err(ImportEnd::Abandoned)
            ),
            "endpoint::hand_preamble for an invocation its host gave up must end the invocation \
             rather than answer"
        );
    }

    // @behavior RT-006
    #[test]
    fn a_guest_exporting_no_versioned_downcall_is_refused() {
        assert!(
            matches!(
                verify_abi_version(["__kobako_downcall", "memory"]),
                Err(SetupError::Dead(message)) if message.contains("exports no kobako:abi/guest@")
            ),
            "a Guest Binary whose exports hold no kobako:abi/guest downcall must be refused, \
             saying none is exported"
        );
    }

    // @behavior RT-007
    #[test]
    fn a_guest_exporting_another_version_is_refused() {
        assert!(
            matches!(
                verify_abi_version(["kobako:abi/guest@0.5.0#downcall"]),
                Err(SetupError::Dead(message)) if message.contains("reports ABI version 0.5.0")
            ),
            "a Guest Binary exporting its downcall under 0.5.0 must be refused, naming the \
             version it reports"
        );
        assert!(
            verify_abi_version([DOWNCALL_EXPORT]).is_ok(),
            "a Guest Binary exporting its downcall under the host's version must be accepted"
        );
    }
}
