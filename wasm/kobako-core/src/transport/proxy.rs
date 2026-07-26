//! Guest transport proxy — the guest-side dispatch pipeline.
//!
//! This module is the glue between the interpreter-side bridge of the
//! consuming guest crate (for the bundled guest: the `method_missing`
//! shims in `kobako-mruby`'s bridge module, whose shared
//! `forward_to_dispatch` body calls `invoke` here) and the wasm-level
//! `__kobako_dispatch` host import declared in `crate::abi`.
//! docs/wire-contract.md § Call Shape / § Reply Shape pins the contract
//! this module implements.
//!
//! `invoke` builds a `Request`, encodes it, calls the host, and
//! demuxes the decoded `Response` — `Ok(value)` back to the bridge,
//! `Err(payload)` into the exception the bridge raises. The envelope
//! codec is already pinned at the value-object layer (`request.rs` /
//! `response.rs` golden vectors); on the host target a thread-local
//! loopback hook stands in for `__kobako_dispatch` so the demux logic
//! tests without a real wasm runtime.

#[cfg(target_arch = "wasm32")]
use crate::abi::__kobako_dispatch;
#[cfg(target_arch = "wasm32")]
use crate::abi::unpack_u64;
use kobako_codec::codec::{self, Decoder, Encode, Value};
use kobako_codec::envelope::{self, Call, Reply, Target};
use kobako_codec::payload::Arguments;

// ---------------------------------------------------------------------
// Exception payload returned to mruby on the error path.
// ---------------------------------------------------------------------

/// The shape of a Response.err payload after envelope-level decoding —
/// exactly the fields the consuming bridge needs to raise the guest
/// exception (SPEC pins every Response.err to the single guest-side
/// `Kobako::ServiceError`, so nothing beyond `kind` and `message` is
/// carried).
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

impl From<envelope::Error> for InvokeError {
    /// A malformed core envelope reaches mruby the same way a malformed
    /// payload does: the guest cannot tell the two apart at the call site,
    /// and both mean the host answered with something unusable.
    fn from(e: envelope::Error) -> Self {
        InvokeError::Codec(codec::Error::Malformed(e.0))
    }
}

impl std::fmt::Display for InvokeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InvokeError::Service(ex) => {
                write!(f, "service raised {}: {}", ex.kind, ex.message)
            }
            InvokeError::Codec(e) => write!(f, "Sandbox communication error: {e}"),
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

/// Invoke the host via `__kobako_dispatch` (or the loopback hook on
/// host targets). On success, returns the value out of the Reply's ok
/// arm; on the fault arm returns `InvokeError::Service`; on a wire fault
/// returns `InvokeError::Codec`.
pub fn invoke(
    target: Target,
    method: &str,
    args: &[Value],
    kwargs: &[(String, Value)],
    block_given: bool,
) -> Result<Value, InvokeError> {
    let call = Call {
        target,
        method: method.to_string(),
        block_given,
        payload: Arguments::new(args.to_vec(), kwargs.to_vec()).encode()?,
    };
    let reply_bytes = host_call(&call.encode())?;
    classify_reply(Reply::decode(&reply_bytes)?)
}

/// Demux a decoded Reply into the `invoke` return type. The arm comes
/// from the envelope's tag, so the guest knows whether the Service
/// returned or failed before decoding a single payload byte.
fn classify_reply(reply: Reply) -> Result<Value, InvokeError> {
    match reply {
        Reply::Ok(body) => Ok(Decoder::new(&body).read_only_value()?),
        Reply::Fault(body) => {
            // The fault body is the adapter's encoding of a Fault — an
            // ext 0x02 frame whose payload is the {type, message,
            // details} map.
            let fault = Decoder::new(&body).read_only_value()?;
            let Value::ErrEnv(inner_bytes) = fault else {
                return Err(InvokeError::Codec(codec::Error::Malformed(
                    "the fault arm of a Reply must carry a Fault (ext 0x02)",
                )));
            };
            let mut dec = Decoder::new(&inner_bytes);
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
            Err(InvokeError::Service(ExceptionPayload { kind, message }))
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
// Tests — fast tier (host target, always runs).
// ---------------------------------------------------------------------

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use kobako_codec::codec::Encoder;

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

    /// A Reply's fault body: the adapter's encoding of a Fault, which is
    /// an ext 0x02 frame wrapping the `{type, message, details}` map.
    fn fault_body(typ: &str, message: &str) -> Vec<u8> {
        let mut inner = Encoder::new();
        inner
            .write_value(&Value::Map(vec![
                (Value::Str("type".into()), Value::Str(typ.into())),
                (Value::Str("message".into()), Value::Str(message.into())),
                (Value::Str("details".into()), Value::Nil),
            ]))
            .unwrap();
        Encoder::encode(&Value::ErrEnv(inner.into_bytes())).unwrap()
    }

    /// A Reply's ok body: the adapter's encoding of one return value.
    fn ok_body(value: Value) -> Vec<u8> {
        Encoder::encode(&value).unwrap()
    }

    // ---- invoke demux ----

    #[test]
    fn invoke_returns_the_value_out_of_the_reply_ok_arm() {
        let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        install_canned(
            captured.clone(),
            Reply::Ok(ok_body(Value::Int(42))).encode(),
        );

        let out = invoke(
            Target::Path("MyService::Counter".into()),
            "value",
            &[],
            &[],
            false,
        );
        clear_loopback();

        assert_eq!(
            out,
            Ok(Value::Int(42)),
            "a Reply on its ok arm through invoke must yield the decoded return value"
        );

        let expected = Call {
            target: Target::Path("MyService::Counter".into()),
            method: "value".into(),
            block_given: false,
            payload: Arguments::default().encode().unwrap(),
        }
        .encode();
        assert_eq!(
            *captured.lock().unwrap(),
            expected,
            "the bytes invoke writes must be exactly the Call envelope carrying an Arguments payload"
        );
    }

    #[test]
    fn a_handle_target_rides_the_envelope_rather_than_the_payload() {
        let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        install_canned(
            captured.clone(),
            Reply::Ok(ok_body(Value::Str("ok".into()))).encode(),
        );

        let out = invoke(
            Target::Handle(7),
            "commit",
            &[Value::Bool(true)],
            &[("force".into(), Value::Bool(false))],
            false,
        );
        clear_loopback();

        assert_eq!(
            out,
            Ok(Value::Str("ok".into())),
            "a Handle-targeted call through invoke must return the Service's value"
        );
        let sent = captured.lock().unwrap().clone();
        assert_eq!(
            sent[0..5],
            [1, 0, 0, 0, 7],
            "a Handle target must ride the envelope's kind tag and bare id, \
             reachable without decoding the payload"
        );
    }

    #[test]
    fn invoke_returns_a_service_error_on_the_reply_fault_arm() {
        let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        install_canned(
            captured,
            Reply::Fault(fault_body("runtime", "boom")).encode(),
        );

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
                assert_eq!(
                    (ex.kind.as_str(), ex.message.as_str()),
                    ("runtime", "boom"),
                    "a Reply on its fault arm through invoke must surface the Fault's type and message"
                );
            }
            other => panic!("expected Service, got {other:?}"),
        }
    }

    #[test]
    fn a_malformed_reply_surfaces_as_a_wire_fault() {
        let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        // A tag the envelope does not define, so the failure is caught
        // before any payload byte is read.
        install_canned(captured, vec![0x7f]);

        let out = invoke(Target::Path("G::M".into()), "x", &[], &[], false);
        clear_loopback();

        match out {
            Err(InvokeError::Codec(_)) => {}
            other => panic!("expected Codec error, got {other:?}"),
        }
    }

    #[test]
    fn a_fault_arm_carrying_something_other_than_a_fault_is_refused() {
        let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        install_canned(captured, Reply::Fault(ok_body(Value::Int(1))).encode());

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
