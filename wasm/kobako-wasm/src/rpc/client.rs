//! Guest RPC Client — Rust+mruby bridge.
//!
//! This module is the glue between the in-VM mruby proxy installed
//! by `crate::kobako::Kobako::install` (mruby C API registrations) and
//! the wasm-level `__kobako_dispatch` host import declared in `abi.rs`.
//! SPEC.md "Wire Contract" → Request / Response pins the contract this
//! module implements.
//!
//! ## Layered responsibilities
//!
//! 1. [`build_request_bytes`] — pure encoder. Given a [`Target`],
//!    method, args, and kwargs, produces a Request envelope per
//!    [`crate::rpc::envelope::encode_request`]. Trivially testable on the
//!    host target; cross-checked against the envelope-layer golden
//!    vectors so that any drift surfaces in `cargo test`.
//!
//! 2. [`invoke_rpc`] — full round-trip. Builds the Request bytes, calls
//!    the host via `__kobako_dispatch` on `wasm32`, then decodes the
//!    response. On the host target (`#[cfg(not(target_arch = "wasm32"))]`)
//!    a thread-local **loopback** hook stands in for the host so that
//!    integration-style tests can drive the full RPC path without a
//!    real wasm runtime.
//!
//! ## Why the loopback indirection on host
//!
//! The codec and envelope layers are exhaustively tested on the host
//! target already (see `envelope.rs` and `codec/mod.rs`). What this
//! module adds is the *demux* logic that turns a `Response::Ok(value)`
//! into a returnable mruby value, and a `Response::Err(payload)` into
//! an mruby exception. We test that demux without a real wasm runtime
//! by feeding the function a canned response via the loopback hook.
//!
//! ## Where the mruby C-side bridge lives
//!
//! User-script RPC calls land in C via the `Kobako::RPC::Client` singleton-class
//! `method_missing` shim (and `Kobako::RPC::Handle#method_missing` for the
//! Handle chaining path, SPEC.md B-17). Both shims live in
//! `crate::kobako::bridges` and call into `Kobako::dispatch_invoke`,
//! which in turn calls [`invoke_rpc`] here. This module's role is the
//! Rust-level encode/transport/decode pipeline that the C bridges
//! delegate to.

#[cfg(target_arch = "wasm32")]
use crate::abi::__kobako_dispatch;
#[cfg(target_arch = "wasm32")]
use crate::abi::unpack_u64;
use crate::codec::{CodecError, Decoder, Value};
use crate::rpc::envelope::{encode_request, EnvelopeError, Request, Response, Target};

// ---------------------------------------------------------------------
// Exception payload returned to mruby on the error path.
// ---------------------------------------------------------------------

/// The shape of a Response.err payload after envelope-level decoding.
/// The mruby C-bridge raises this as a `Kobako::ServiceError` /
/// host-mapped exception; the bridge does not need to inspect more than
/// `kind` and `message` to do so.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExceptionPayload {
    /// The wire `type` field of the inner ext 0x02 map (e.g.
    /// `"runtime"`, `"undefined"`). Named `kind` on the Rust side to
    /// avoid the raw-identifier escape for the `type` keyword.
    /// SPEC.md → "Error Envelope".
    pub kind: String,
    /// Human-readable message (`message` field of the inner map).
    pub message: String,
    /// Raw payload bytes — preserved so the mruby bridge can hand them
    /// back to Ruby code that wants to inspect `details` without
    /// re-decoding.
    pub raw: Vec<u8>,
}

/// Error variants returned by [`invoke_rpc`].
///
/// `ServiceErr` carries the SPEC-mandated Response.err path payload;
/// `Wire` covers everything that fails *before* the response can be
/// classified (envelope shape violations, codec faults, host returning
/// `len == 0`).
#[derive(Debug, Clone, PartialEq)]
pub enum InvokeError {
    /// The host returned a Response.err — this is the *normal* path for
    /// a Service raising an exception, surfaced to mruby as a re-raise.
    ServiceErr(ExceptionPayload),
    /// A wire-layer fault — host returned malformed bytes, the response
    /// was not a Response envelope, or the host signalled `len == 0`.
    /// In a real run this routes to `Kobako::SandboxError` / `TrapError`
    /// via the boot script's panic path.
    Wire(EnvelopeError),
}

impl From<EnvelopeError> for InvokeError {
    fn from(e: EnvelopeError) -> Self {
        InvokeError::Wire(e)
    }
}

impl From<CodecError> for InvokeError {
    fn from(e: CodecError) -> Self {
        InvokeError::Wire(EnvelopeError::Codec(e))
    }
}

impl std::fmt::Display for InvokeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InvokeError::ServiceErr(ex) => {
                write!(f, "service raised {}: {}", ex.kind, ex.message)
            }
            InvokeError::Wire(e) => write!(f, "RPC wire fault: {e}"),
        }
    }
}

impl std::error::Error for InvokeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            InvokeError::Wire(e) => Some(e),
            InvokeError::ServiceErr(_) => None,
        }
    }
}

// ---------------------------------------------------------------------
// Pure Request builder — decoupled from the host import for testing.
// ---------------------------------------------------------------------

/// Build the bytes of a Request envelope from the four-tuple every RPC
/// site provides (target, method, args, kwargs). Pure function; does
/// not touch wasm linear memory or the host import.
///
/// SPEC reference: SPEC.md → Wire Codec → Request (4-element array,
/// encoded narrowest).
pub fn build_request_bytes(
    target: Target,
    method: &str,
    args: &[Value],
    kwargs: &[(String, Value)],
) -> Result<Vec<u8>, EnvelopeError> {
    let req = Request {
        target,
        method: method.to_string(),
        args: args.to_vec(),
        kwargs: kwargs.to_vec(),
    };
    encode_request(&req)
}

// ---------------------------------------------------------------------
// Full RPC round-trip with loopback hook for host-target tests.
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
/// on a Response.err path returns [`InvokeError::ServiceErr`]; on a
/// wire fault returns [`InvokeError::Wire`].
pub fn invoke_rpc(
    target: Target,
    method: &str,
    args: &[Value],
    kwargs: &[(String, Value)],
) -> Result<Value, InvokeError> {
    let req_bytes = build_request_bytes(target, method, args, kwargs)?;
    let resp_bytes = host_call(&req_bytes)?;
    let resp = crate::rpc::envelope::decode_response(&resp_bytes)?;
    classify_response(resp)
}

/// Demux a decoded Response into the [`invoke_rpc`] return type.
fn classify_response(resp: Response) -> Result<Value, InvokeError> {
    match resp {
        Response::Ok(v) => Ok(v),
        Response::Err(payload_bytes) => {
            // Decode the inner ext 0x02 Exception map: {type, message, details}.
            let mut dec = Decoder::new(&payload_bytes);
            let inner = dec
                .read_value()
                .map_err(|e| InvokeError::Wire(EnvelopeError::Codec(e)))?;
            let pairs = match inner {
                Value::Map(p) => p,
                _ => {
                    return Err(InvokeError::Wire(EnvelopeError::WrongFieldType(
                        "ErrEnv inner payload must be a map",
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
            let kind = typ.ok_or(InvokeError::Wire(EnvelopeError::MissingField("type")))?;
            let message = msg.ok_or(InvokeError::Wire(EnvelopeError::MissingField("message")))?;
            Err(InvokeError::ServiceErr(ExceptionPayload {
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
    // On wasm32, write the request into linear memory at a stable
    // address and call the host import. The host writes the response
    // into a buffer it allocated via `__kobako_alloc` and returns the
    // packed (ptr, len) tuple.
    //
    // Item #11 (allocator) and #12 (host linker) finish wiring this
    // path. For now we hold the request in a Box leaking the bytes for
    // the duration of the call; once the allocator is real this becomes
    // a `__kobako_alloc(req.len())` round trip.
    let req_ptr = req_bytes.as_ptr() as u32;
    let req_len = req_bytes.len() as u32;
    let packed = unsafe { __kobako_dispatch(req_ptr, req_len) };
    let (ptr, len) = unpack_u64(packed);
    if len == 0 {
        // Wire violation per SPEC.md → ABI Signatures.
        return Err(InvokeError::Wire(EnvelopeError::Shape(
            "host returned len == 0",
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
        None => Err(InvokeError::Wire(EnvelopeError::Shape(
            "no loopback hook installed; install one with set_loopback() \
             when calling invoke_rpc on the host target",
        ))),
    })
}

// ---------------------------------------------------------------------
// mruby C-bridge — see `crate::kobako::bridges`.
// ---------------------------------------------------------------------
//
// The C-side dispatch entries are the `Kobako::RPC::Client` singleton-class
// `method_missing` shim and `Kobako::RPC::Handle#method_missing`. Both
// live in `crate::kobako::bridges` and reach this module through
// `Kobako::dispatch_invoke`.

// ---------------------------------------------------------------------
// Tests — fast tier (host target, always runs).
// ---------------------------------------------------------------------

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::codec::Encoder;
    use crate::rpc::envelope::{encode_request, encode_response, Response};

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

    // ---- build_request_bytes ----

    #[test]
    fn build_request_bytes_matches_envelope_encoder() {
        // SPEC cross-check: build_request_bytes must produce byte-
        // identical output to encode_request for the same logical
        // Request. This is the contract that lets envelope-layer golden
        // vectors transitively cover this module.
        let target = Target::Path("MyService::Logger".into());
        let method = "info";
        let args = vec![Value::Str("hello".into())];
        let kwargs: Vec<(String, Value)> = vec![];

        let direct = build_request_bytes(target.clone(), method, &args, &kwargs).unwrap();
        let viaenv = encode_request(&Request {
            target,
            method: method.into(),
            args,
            kwargs,
        })
        .unwrap();
        assert_eq!(direct, viaenv);
    }

    #[test]
    fn build_request_bytes_empty_args_and_kwargs_golden() {
        // Golden vector — same hex as the envelope-layer
        // `request_golden_empty_args_and_kwargs` test; if either layer
        // drifts, both tests fail simultaneously and the discrepancy is
        // immediately localised.
        let bytes = build_request_bytes(Target::Path("G::M".into()), "ping", &[], &[]).unwrap();
        assert_eq!(
            bytes,
            vec![
                0x94, // fixarray 4
                0xa4, b'G', b':', b':', b'M', 0xa4, b'p', b'i', b'n', b'g',
                0x90, // fixarray 0
                0x80, // fixmap 0
            ]
        );
    }

    // ---- invoke_rpc demux ----

    #[test]
    fn invoke_rpc_returns_value_on_response_ok() {
        let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let response = encode_response(&Response::Ok(Value::Int(42))).unwrap();
        install_canned(captured.clone(), response);

        let out = invoke_rpc(Target::Path("MyService::Counter".into()), "value", &[], &[]);
        clear_loopback();

        assert_eq!(out, Ok(Value::Int(42)));

        // Cross-check: captured bytes are exactly what the envelope
        // encoder would have produced for this Request.
        let expected = encode_request(&Request {
            target: Target::Path("MyService::Counter".into()),
            method: "value".into(),
            args: vec![],
            kwargs: vec![],
        })
        .unwrap();
        assert_eq!(*captured.lock().unwrap(), expected);
    }

    #[test]
    fn invoke_rpc_handle_target_round_trip() {
        let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let response = encode_response(&Response::Ok(Value::Str("ok".into()))).unwrap();
        install_canned(captured.clone(), response);

        let out = invoke_rpc(
            Target::Handle(7),
            "commit",
            &[Value::Bool(true)],
            &[("force".into(), Value::Bool(false))],
        );
        clear_loopback();

        assert_eq!(out, Ok(Value::Str("ok".into())));
        // Spot-check: first byte indicates fixarray 4 envelope; second
        // byte is the ext 0x01 Handle marker (`0xd6`), proving the
        // Handle target was encoded as ext rather than str.
        let req = captured.lock().unwrap().clone();
        assert_eq!(req[0], 0x94, "fixarray 4 envelope");
        assert_eq!(req[1], 0xd6, "fixext 4 marker for Handle target");
    }

    #[test]
    fn invoke_rpc_returns_service_err_on_response_err() {
        let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let response = encode_response(&Response::Err(errenv_payload("runtime", "boom"))).unwrap();
        install_canned(captured, response);

        let out = invoke_rpc(
            Target::Path("MyService::KV".into()),
            "get",
            &[Value::Str("missing".into())],
            &[],
        );
        clear_loopback();

        match out {
            Err(InvokeError::ServiceErr(ex)) => {
                assert_eq!(ex.kind, "runtime");
                assert_eq!(ex.message, "boom");
                assert!(!ex.raw.is_empty(), "raw payload bytes must be preserved");
            }
            other => panic!("expected ServiceErr, got {other:?}"),
        }
    }

    #[test]
    fn invoke_rpc_propagates_wire_error_on_garbage_response() {
        let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        // Garbage: a single 0xc1 byte (reserved msgpack family).
        install_canned(captured, vec![0xc1]);

        let out = invoke_rpc(Target::Path("G::M".into()), "x", &[], &[]);
        clear_loopback();

        match out {
            Err(InvokeError::Wire(_)) => {}
            other => panic!("expected Wire error, got {other:?}"),
        }
    }

    #[test]
    fn invoke_rpc_without_loopback_returns_wire_error() {
        // Defensive: if a test forgets to install a loopback, the
        // function must fail loudly rather than block or panic.
        clear_loopback();
        let out = invoke_rpc(Target::Path("G::M".into()), "x", &[], &[]);
        match out {
            Err(InvokeError::Wire(EnvelopeError::Shape(msg))) => {
                assert!(msg.contains("loopback"), "unexpected message: {msg}");
            }
            other => panic!("expected Wire(Shape) error, got {other:?}"),
        }
    }
}
