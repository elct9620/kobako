//! Guest transport proxy — Rust+mruby bridge.
//!
//! This module is the glue between the in-VM mruby proxy installed
//! by `crate::kobako::Kobako::install` (mruby C API registrations) and
//! the wasm-level `__kobako_dispatch` host import declared in `abi.rs`.
//! docs/wire-contract.md § Request / Response pins the contract this
//! module implements.
//!
//! ## Layered responsibilities
//!
//! `invoke` — full round-trip. Builds a `Request`, encodes it via
//! its `crate::codec::Encode` impl, calls the host via
//! `__kobako_dispatch` on `wasm32`, then decodes the `Response`. On the
//! host target (`#[cfg(not(target_arch = "wasm32"))]`) a thread-local
//! **loopback** hook stands in for the host so that integration-style
//! tests can drive the full transport path without a real wasm runtime.
//! The envelope codec itself is exhaustively tested at the value-object
//! layer (`request.rs` / `response.rs` golden vectors); this module only
//! adds the demux logic.
//!
//! ## Why the loopback indirection on host
//!
//! The codec and envelope layers are exhaustively tested on the host
//! target already (see `request.rs` / `response.rs` and `codec/mod.rs`).
//! What this
//! module adds is the *demux* logic that turns a `Response::Ok(value)`
//! into a returnable mruby value, and a `Response::Err(payload)` into
//! an mruby exception. We test that demux without a real wasm runtime
//! by feeding the function a canned response via the loopback hook.
//!
//! ## Where the mruby C-side bridge lives
//!
//! User-script transport calls land in C via two `method_missing`
//! shims, one per `Kobako::Transport::Proxy` subclass: the singleton-class
//! shim on `Kobako::Member` (Member classes) and the instance shim on
//! `Kobako::Handle` for the Handle chaining path (docs/behavior.md B-17).
//! Both shims live in `crate::kobako::bridges`; their shared
//! `forward_to_dispatch` body calls `invoke` here. This module's role
//! is the Rust-level encode/transport/decode pipeline that the C bridges
//! delegate to.

#[cfg(target_arch = "wasm32")]
use crate::abi::__kobako_dispatch;
#[cfg(target_arch = "wasm32")]
use crate::abi::unpack_u64;
use crate::codec::{self, Decode, Decoder, Encode, Value};
use crate::transport::{Request, Response, Target};

// ---------------------------------------------------------------------
// Exception payload returned to mruby on the error path.
// ---------------------------------------------------------------------

/// The shape of a Response.err payload after envelope-level decoding.
/// The mruby C-bridge raises this as a `Kobako::ServiceError` /
/// host-mapped exception; the bridge does not need to inspect more than
/// `kind` and `message` to do so.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExceptionPayload {
    /// The envelope `type` field of the inner ext 0x02 map (e.g.
    /// `"runtime"`, `"undefined"`). Named `kind` on the Rust side to
    /// avoid the raw-identifier escape for the `type` keyword.
    /// docs/wire-contract.md § Fault Envelope pins the field shape;
    /// the reserved `type` values are governed by SPEC.md § Error
    /// Classes.
    pub kind: String,
    /// Human-readable message (`message` field of the inner map).
    pub message: String,
    /// Raw payload bytes — preserved so the mruby bridge can hand them
    /// back to Ruby code that wants to inspect `details` without
    /// re-decoding.
    pub raw: Vec<u8>,
}

/// Error variants returned by `invoke`.
///
/// `Service` carries the SPEC-mandated Response.err path payload;
/// `Codec` covers everything that fails *before* the response can be
/// classified (wire-shape violations, codec faults, host returning
/// `len == 0`).
#[derive(Debug, Clone, PartialEq)]
pub enum InvokeError {
    /// The host returned a Response.err — this is the *normal* path for
    /// a Service raising an exception, surfaced to mruby as a re-raise.
    Service(ExceptionPayload),
    /// A wire-layer fault — host returned malformed bytes, the response
    /// was not a Response envelope, or the host signalled `len == 0`. In
    /// a real run this routes to `Kobako::SandboxError` / `TrapError` via
    /// the boot script's panic path.
    Codec(codec::Error),
}

impl From<codec::Error> for InvokeError {
    fn from(e: codec::Error) -> Self {
        InvokeError::Codec(e)
    }
}

impl std::fmt::Display for InvokeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InvokeError::Service(ex) => {
                write!(f, "service raised {}: {}", ex.kind, ex.message)
            }
            InvokeError::Codec(e) => write!(f, "sandbox communication error: {e}"),
        }
    }
}

impl std::error::Error for InvokeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            InvokeError::Codec(e) => Some(e),
            InvokeError::Service(_) => None,
        }
    }
}

// ---------------------------------------------------------------------
// Full transport round-trip with loopback hook for host-target tests.
// ---------------------------------------------------------------------

/// Function signature for the host-target loopback. Receives the
/// *Request bytes* the caller would have written into wasm linear
/// memory and returns the Response bytes the host would have written
/// back via `__kobako_alloc`. Pure in/out; no shared state.
#[cfg(not(target_arch = "wasm32"))]
pub type LoopbackFn = Box<dyn Fn(&[u8]) -> Vec<u8> + Send + 'static>;

#[cfg(not(target_arch = "wasm32"))]
thread_local! {
    static LOOPBACK: std::cell::RefCell<Option<LoopbackFn>> =
        const { std::cell::RefCell::new(None) };
}

/// Install a loopback hook for the current thread. Returns the previous
/// hook so test scaffolding can stack and restore.
#[cfg(not(target_arch = "wasm32"))]
pub fn set_loopback(hook: Option<LoopbackFn>) -> Option<LoopbackFn> {
    LOOPBACK.with(|cell| std::mem::replace(&mut *cell.borrow_mut(), hook))
}

/// Invoke the host via `__kobako_dispatch` (or the loopback hook on
/// host targets). On success, returns the value out of `Response::Ok`;
/// on a Response.err path returns `InvokeError::Service`; on an
/// wire fault returns `InvokeError::Codec`.
pub fn invoke(
    target: Target,
    method: &str,
    args: &[Value],
    kwargs: &[(String, Value)],
    block_given: bool,
) -> Result<Value, InvokeError> {
    let req = Request {
        target,
        method: method.to_string(),
        args: args.to_vec(),
        kwargs: kwargs.to_vec(),
        block_given,
    };
    let req_bytes = req.encode()?;
    let resp_bytes = host_call(&req_bytes)?;
    let resp = Response::decode(&resp_bytes)?;
    classify_response(resp)
}

/// Demux a decoded Response into the `invoke` return type.
fn classify_response(resp: Response) -> Result<Value, InvokeError> {
    match resp {
        Response::Ok(v) => Ok(v),
        Response::Err(payload_bytes) => {
            // Decode the inner ext 0x02 Exception map: {type, message, details}.
            let mut dec = Decoder::new(&payload_bytes);
            let inner = dec.read_value()?;
            let pairs = match inner {
                Value::Map(p) => p,
                _ => {
                    return Err(InvokeError::Codec(codec::Error::Malformed(
                        "malformed error response from the host",
                    )));
                }
            };
            let mut typ = None;
            let mut msg = None;
            for (k, v) in pairs {
                if let Value::Str(name) = k {
                    match name.as_str() {
                        "type" => {
                            if let Value::Str(s) = v {
                                typ = Some(s);
                            }
                        }
                        "message" => {
                            if let Value::Str(s) = v {
                                msg = Some(s);
                            }
                        }
                        _ => {}
                    }
                }
            }
            let kind = typ.ok_or(InvokeError::Codec(codec::Error::Malformed(
                "error response from the host is missing the field: type",
            )))?;
            let message = msg.ok_or(InvokeError::Codec(codec::Error::Malformed(
                "error response from the host is missing the field: message",
            )))?;
            Err(InvokeError::Service(ExceptionPayload {
                kind,
                message,
                raw: payload_bytes,
            }))
        }
    }
}

// ---------------------------------------------------------------------
// host_call — the only function that differs between wasm32 and host.
// ---------------------------------------------------------------------

#[cfg(target_arch = "wasm32")]
fn host_call(req_bytes: &[u8]) -> Result<Vec<u8>, InvokeError> {
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
        return Err(InvokeError::Codec(codec::Error::Malformed(
            "the host returned an empty response",
        )));
    }
    // SAFETY: the host promises [ptr, ptr+len) is a valid response
    // buffer in our linear memory for the duration of this call frame.
    let slice = unsafe { core::slice::from_raw_parts(ptr as *const u8, len as usize) };
    Ok(slice.to_vec())
}

#[cfg(not(target_arch = "wasm32"))]
fn host_call(req_bytes: &[u8]) -> Result<Vec<u8>, InvokeError> {
    LOOPBACK.with(|cell| match cell.borrow().as_ref() {
        Some(hook) => Ok(hook(req_bytes)),
        None => Err(InvokeError::Codec(codec::Error::Malformed(
            "no loopback hook installed; install one with set_loopback() \
             when calling invoke on the host target",
        ))),
    })
}

// ---------------------------------------------------------------------
// mruby C-bridge — see `crate::kobako::bridges`.
// ---------------------------------------------------------------------
//
// The C-side dispatch entries are the `Kobako::Member` singleton-class
// `method_missing` shim and the `Kobako::Handle` instance `method_missing`
// shim. Both live in `crate::kobako::bridges` and reach this module
// through their shared `forward_to_dispatch` body.

// ---------------------------------------------------------------------
// Tests — fast tier (host target, always runs).
// ---------------------------------------------------------------------

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::codec::Encoder;

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

    fn errenv_payload(typ: &str, message: &str) -> Vec<u8> {
        let mut enc = Encoder::new();
        enc.write_value(&Value::Map(vec![
            (Value::Str("type".into()), Value::Str(typ.into())),
            (Value::Str("message".into()), Value::Str(message.into())),
            (Value::Str("details".into()), Value::Nil),
        ]))
        .unwrap();
        enc.into_bytes()
    }

    // ---- invoke demux ----

    #[test]
    fn invoke_returns_value_on_response_ok() {
        let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let response = Response::Ok(Value::Int(42)).encode().unwrap();
        install_canned(captured.clone(), response);

        let out = invoke(
            Target::Path("MyService::Counter".into()),
            "value",
            &[],
            &[],
            false,
        );
        clear_loopback();

        assert_eq!(out, Ok(Value::Int(42)));

        // Cross-check: captured bytes are exactly what the envelope
        // encoder would have produced for this Request.
        let expected = Request {
            target: Target::Path("MyService::Counter".into()),
            method: "value".into(),
            args: vec![],
            kwargs: vec![],
            block_given: false,
        }
        .encode()
        .unwrap();
        assert_eq!(*captured.lock().unwrap(), expected);
    }

    #[test]
    fn invoke_handle_target_round_trip() {
        let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let response = Response::Ok(Value::Str("ok".into())).encode().unwrap();
        install_canned(captured.clone(), response);

        let out = invoke(
            Target::Handle(7),
            "commit",
            &[Value::Bool(true)],
            &[("force".into(), Value::Bool(false))],
            false,
        );
        clear_loopback();

        assert_eq!(out, Ok(Value::Str("ok".into())));
        // Spot-check: first byte indicates fixarray 5 envelope; second
        // byte is the ext 0x01 Handle marker (`0xd6`), proving the
        // Handle target was encoded as ext rather than str.
        let req = captured.lock().unwrap().clone();
        assert_eq!(req[0], 0x95, "fixarray 5 envelope");
        assert_eq!(req[1], 0xd6, "fixext 4 marker for Handle target");
    }

    #[test]
    fn invoke_returns_service_err_on_response_err() {
        let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let response = Response::Err(errenv_payload("runtime", "boom"))
            .encode()
            .unwrap();
        install_canned(captured, response);

        let out = invoke(
            Target::Path("MyService::KV".into()),
            "get",
            &[Value::Str("missing".into())],
            &[],
            false,
        );
        clear_loopback();

        match out {
            Err(InvokeError::Service(ex)) => {
                assert_eq!(ex.kind, "runtime");
                assert_eq!(ex.message, "boom");
                assert!(!ex.raw.is_empty(), "raw payload bytes must be preserved");
            }
            other => panic!("expected Service, got {other:?}"),
        }
    }

    #[test]
    fn invoke_propagates_envelope_error_on_garbage_response() {
        let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        // Garbage: a single 0xc1 byte (reserved msgpack family).
        install_canned(captured, vec![0xc1]);

        let out = invoke(Target::Path("G::M".into()), "x", &[], &[], false);
        clear_loopback();

        match out {
            Err(InvokeError::Codec(_)) => {}
            other => panic!("expected Codec error, got {other:?}"),
        }
    }

    #[test]
    fn invoke_without_loopback_returns_envelope_error() {
        // Defensive: if a test forgets to install a loopback, the
        // function must fail loudly rather than block or panic.
        clear_loopback();
        let out = invoke(Target::Path("G::M".into()), "x", &[], &[], false);
        match out {
            Err(InvokeError::Codec(codec::Error::Malformed(msg))) => {
                assert!(msg.contains("loopback"), "unexpected message: {msg}");
            }
            other => panic!("expected Codec(Malformed) error, got {other:?}"),
        }
    }
}
