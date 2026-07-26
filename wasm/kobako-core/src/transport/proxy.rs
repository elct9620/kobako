//! Guest transport proxy — the guest-side dispatch pipeline.
//!
//! This module is the glue between the interpreter-side bridge of the
//! consuming guest crate (for the bundled guest: the `method_missing`
//! shims in `kobako-mruby`'s bridge module, whose shared
//! `forward_to_dispatch` body calls `dispatch` here) and the wasm-level
//! `__kobako_dispatch` host import declared in `crate::abi`.
//! docs/wire-contract.md § Call Shape / § Reply Shape pins the contract
//! this module implements.
//!
//! `dispatch` builds a `Call` around a payload it never reads, calls the
//! host, and hands back the arm the Reply's tag named — the ok body to the
//! caller, the fault body as an error. What either body means is the
//! payload adapter's business, which is what lets a guest with its own
//! schema drive this layer with no MessagePack in its dependency graph.
//! On the host target a thread-local loopback hook stands in for
//! `__kobako_dispatch` so the demux logic tests without a real wasm
//! runtime.

#[cfg(target_arch = "wasm32")]
use crate::abi::__kobako_dispatch;
#[cfg(target_arch = "wasm32")]
use crate::abi::unpack_u64;
use kobako_codec::envelope::{self, Call, Reply, Target};

/// Why a dispatch came back without a value.
///
/// Non-exhaustive: a guest matches on this, and a later way for the
/// exchange to fail should not break the ones already written. Keep a
/// wildcard arm and treat it the way `Wire` is treated — anything that is
/// not the Service's own fault means the exchange did not complete.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum DispatchError {
    /// The host answered on the fault arm — the *normal* path for a
    /// Service raising an exception. The bytes are the fault body as the
    /// payload adapter encoded it; reading them is the caller's business.
    Fault(Vec<u8>),
    /// The exchange failed before a Reply could be framed: malformed
    /// bytes, an answer that is not a Reply envelope, or the host
    /// signalling `len == 0`.
    Wire(envelope::Error),
}

impl From<envelope::Error> for DispatchError {
    fn from(err: envelope::Error) -> Self {
        DispatchError::Wire(err)
    }
}

impl std::fmt::Display for DispatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DispatchError::Fault(_) => f.write_str("the Service answered on the fault arm"),
            DispatchError::Wire(err) => write!(f, "Sandbox communication error: {err}"),
        }
    }
}

impl std::error::Error for DispatchError {}

// ---------------------------------------------------------------------
// Full transport round-trip with loopback hook for host-target tests.
// ---------------------------------------------------------------------

/// Function signature for the host-target loopback. Receives the
/// *Call bytes* the caller would have written into wasm linear
/// memory and returns the Reply bytes the host would have written
/// back via `__kobako_alloc`. Pure in/out; no shared state.
#[cfg(not(target_arch = "wasm32"))]
type LoopbackFn = Box<dyn Fn(&[u8]) -> Vec<u8> + Send + 'static>;

#[cfg(not(target_arch = "wasm32"))]
thread_local! {
    static LOOPBACK: std::cell::RefCell<Option<LoopbackFn>> =
        const { std::cell::RefCell::new(None) };
}

/// Install a loopback hook for the current thread. Returns the previous
/// hook so test scaffolding can stack and restore. Test-only — the
/// loopback is the host-target stand-in for the wasm `__kobako_dispatch`
/// import, exercised by this module's unit tests.
#[cfg(all(test, not(target_arch = "wasm32")))]
fn set_loopback(hook: Option<LoopbackFn>) -> Option<LoopbackFn> {
    LOOPBACK.with(|cell| std::mem::replace(&mut *cell.borrow_mut(), hook))
}

/// Route one Call to the host and answer with the body its Reply tagged.
///
/// `payload` crosses untouched: the arm comes off the envelope's tag, so
/// the guest learns whether the Service returned or failed before any
/// schema reads a byte.
pub fn dispatch(
    target: Target,
    method: &str,
    block_given: bool,
    payload: &[u8],
) -> Result<Vec<u8>, DispatchError> {
    let call = Call {
        target,
        method: method.to_string(),
        block_given,
        payload: payload.to_vec(),
    };
    let reply_bytes = host_call(&call.encode())?;
    match Reply::decode(&reply_bytes)? {
        Reply::Ok(body) => Ok(body),
        Reply::Fault(body) => Err(DispatchError::Fault(body)),
    }
}

// ---------------------------------------------------------------------
// host_call — the only function that differs between wasm32 and host.
// ---------------------------------------------------------------------

#[cfg(target_arch = "wasm32")]
fn host_call(req_bytes: &[u8]) -> Result<Vec<u8>, DispatchError> {
    // On wasm32, pass the request by its current linear-memory address
    // and call the host import. The host reads `[req_ptr, req_ptr+len)`
    // out of our memory, writes the response into a buffer it allocated
    // via `__kobako_alloc`, and returns the packed (ptr, len) tuple.
    //
    // The request bytes stay live for the synchronous `__kobako_dispatch`
    // call because `req_bytes` is borrowed by this frame, which is parked
    // on the wasm stack until the host returns — no copy into a guest
    // buffer is needed on the request side.
    let req_ptr = req_bytes.as_ptr() as u32;
    let req_len = req_bytes.len() as u32;
    let packed = unsafe { __kobako_dispatch(req_ptr, req_len) };
    let (ptr, len) = unpack_u64(packed);
    if len == 0 {
        // Wire violation per docs/wire-codec.md § ABI Signatures.
        return Err(DispatchError::Wire(envelope::Error(
            "the host returned an empty response",
        )));
    }
    // SAFETY: the host promises [ptr, ptr+len) is a valid response
    // buffer in our linear memory for the duration of this call frame.
    let slice = unsafe { core::slice::from_raw_parts(ptr as *const u8, len as usize) };
    Ok(slice.to_vec())
}

#[cfg(not(target_arch = "wasm32"))]
fn host_call(req_bytes: &[u8]) -> Result<Vec<u8>, DispatchError> {
    LOOPBACK.with(|cell| match cell.borrow().as_ref() {
        Some(hook) => Ok(hook(req_bytes)),
        None => Err(DispatchError::Wire(envelope::Error(
            "no loopback hook installed; install one with set_loopback() \
             when calling dispatch on the host target",
        ))),
    })
}

// ---------------------------------------------------------------------
// Tests — fast tier (host target, always runs).
// ---------------------------------------------------------------------

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    /// Bytes standing in for a payload this layer must not interpret —
    /// deliberately not valid under any adapter kobako ships.
    const OPAQUE: &[u8] = &[0xc1, 0x00, 0xff, 0x92];

    /// Helper: install a one-shot loopback that captures the request
    /// bytes and returns a canned response.
    fn install_canned(
        captured: std::sync::Arc<std::sync::Mutex<Vec<u8>>>,
        response_bytes: Vec<u8>,
    ) {
        let cb_captured = captured.clone();
        let hook: LoopbackFn = Box::new(move |req: &[u8]| {
            cb_captured.lock().unwrap().extend_from_slice(req);
            response_bytes.clone()
        });
        set_loopback(Some(hook));
    }

    fn clear_loopback() {
        set_loopback(None);
    }

    #[test]
    fn dispatch_returns_the_ok_body_uninterpreted() {
        let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        install_canned(captured.clone(), Reply::Ok(OPAQUE.to_vec()).encode());

        let out = dispatch(
            Target::Path("MyService::Counter".into()),
            "value",
            false,
            OPAQUE,
        );
        clear_loopback();

        assert_eq!(
            out,
            Ok(OPAQUE.to_vec()),
            "a Reply on its ok arm through dispatch must hand back the body no schema read"
        );

        let expected = Call {
            target: Target::Path("MyService::Counter".into()),
            method: "value".into(),
            block_given: false,
            payload: OPAQUE.to_vec(),
        }
        .encode();
        assert_eq!(
            *captured.lock().unwrap(),
            expected,
            "the bytes dispatch writes must be the Call envelope carrying the payload verbatim"
        );
    }

    #[test]
    fn a_handle_target_rides_the_envelope_rather_than_the_payload() {
        let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        install_canned(captured.clone(), Reply::Ok(Vec::new()).encode());

        let out = dispatch(Target::Handle(7), "commit", false, OPAQUE);
        clear_loopback();

        assert!(out.is_ok(), "a Handle-targeted call must reach the host");
        let sent = captured.lock().unwrap().clone();
        assert_eq!(
            sent[0..5],
            [1, 0, 0, 0, 7],
            "a Handle target must ride the envelope's kind tag and bare id, \
             reachable without decoding the payload"
        );
    }

    #[test]
    fn the_fault_arm_hands_back_its_body_for_the_caller_to_read() {
        let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        install_canned(captured, Reply::Fault(OPAQUE.to_vec()).encode());

        let out = dispatch(Target::Path("MyService::KV".into()), "get", false, &[]);
        clear_loopback();

        assert_eq!(
            out,
            Err(DispatchError::Fault(OPAQUE.to_vec())),
            "a Reply on its fault arm must surface the body unread — its schema is the caller's"
        );
    }

    #[test]
    fn a_reply_the_envelope_cannot_frame_is_a_wire_fault() {
        let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        // A tag the envelope does not define, so the failure is caught
        // before any payload byte is read.
        install_canned(captured, vec![0x7f]);

        let out = dispatch(Target::Path("G::M".into()), "x", false, &[]);
        clear_loopback();

        assert!(
            matches!(out, Err(DispatchError::Wire(_))),
            "a tag the Reply envelope does not define must fail as a wire fault, got {out:?}"
        );
    }

    #[test]
    fn dispatch_without_loopback_fails_loudly() {
        // Defensive: if a test forgets to install a loopback, the
        // function must fail rather than block or panic.
        clear_loopback();
        let out = dispatch(Target::Path("G::M".into()), "x", false, &[]);
        match out {
            Err(DispatchError::Wire(envelope::Error(msg))) => {
                assert!(msg.contains("loopback"), "unexpected message: {msg}");
            }
            other => panic!("expected a wire fault naming the missing loopback, got {other:?}"),
        }
    }
}
