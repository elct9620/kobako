//! Integration coverage for the byte-level payload surface — the entries
//! a host uses when the payload's schema is its own: `RunPayload::from_bytes`
//! and `from_fn`, `Execution::payload`, `Receiver::call`, and
//! `Yielder::call_payload`, along with the failures such a host reports
//! through them in kobako's own attribution.
//!
//! The bundled guest speaks MessagePack, so these tests build that schema's
//! bytes themselves rather than reaching for the `Value` shortcuts. That is
//! the point: what the byte surface promises is that the host owns the
//! encode and decode, and the bytes it hands over ride verbatim. A host
//! whose guest speaks another schema substitutes its own encoder at exactly
//! these call sites.

// Driven through the bundled engine: these cases load a real Guest Binary,
// so they stand only in a build that carries one.
#![cfg(feature = "wasmtime")]

use std::sync::{Arc, Mutex};

use kobako::handles::DetachedHandles;
use kobako::{Error, Execution, Fault, FaultKind, Handles, Receiver, RunPayload, Yielder};
use kobako_codec::msgpack::codec::{Decode, Decoder, Encode, Encoder, Value};
use kobako_codec::msgpack::payload::Arguments;

use crate::common::real_sandbox;

/// The host's own encode of a `run` payload — one positional argument.
fn run_args(value: Value) -> Vec<u8> {
    Arguments::new(vec![value], Vec::new())
        .encode()
        .expect("a single-argument payload always encodes")
}

fn decode(bytes: &[u8]) -> Value {
    Decoder::new(bytes)
        .read_sole_value()
        .expect("the guest answered with readable bytes")
}

/// A Receiver on the byte seam: it records the payload bytes it was handed
/// and answers with bytes it encoded itself, never touching a `Value`
/// shortcut.
struct ByteEcho {
    seen: Mutex<Vec<Vec<u8>>>,
}

impl ByteEcho {
    fn new() -> Arc<Self> {
        Arc::new(ByteEcho {
            seen: Mutex::new(Vec::new()),
        })
    }
}

impl Receiver for ByteEcho {
    fn call(
        &self,
        _method: &str,
        payload: &[u8],
        _block: Option<&mut Yielder<'_>>,
        _handles: &Handles<'_>,
    ) -> Result<Vec<u8>, Fault> {
        self.seen
            .lock()
            .expect("never poisoned")
            .push(payload.to_vec());
        let arguments = Arguments::decode(payload)
            .map_err(|err| Fault::new(FaultKind::Runtime, format!("unreadable payload: {err}")))?;
        let answer = arguments.args.first().cloned().unwrap_or(Value::Nil);
        Encoder::encode(&answer)
            .map_err(|err| Fault::new(FaultKind::Runtime, format!("unencodable: {err}")))
    }
}

// @behavior T-167
#[test]
fn a_run_carries_the_host_s_own_bytes_and_payload_hands_them_back() {
    let Some(mut sandbox) = real_sandbox() else {
        return;
    };
    sandbox
        .preload("Echo", "class Echo; def self.call(value) = value; end")
        .expect("preload the entrypoint");

    let execution = sandbox
        .run(
            "Echo",
            RunPayload::from_bytes(run_args(Value::Str("hi".into()))),
        )
        .expect("the invocation runs");

    let bytes = execution
        .payload()
        .expect("the guest returned rather than failing");
    assert_eq!(
        decode(bytes),
        Value::Str("hi".into()),
        "a run payload the host encoded itself must reach the entrypoint, and its \
         answer must reach the host as bytes the host decodes itself"
    );
}

/// A `Receiver` is an interface implemented outside this crate, so it has
/// to be answerable outside an invocation too — otherwise its author can
/// only reach their own method by driving a whole guest. This is the
/// shape that makes it reachable, exercised here on the byte seam because
/// that is where an implementation with its own schema lives.
// @behavior T-168
#[test]
fn a_receiver_answers_through_a_detached_handle_table() {
    let echo = ByteEcho::new();
    let table = DetachedHandles::new();

    let answer = echo
        .call("call", &run_args(Value::Int(7)), None, &table.as_handles())
        .expect("the Receiver answers");

    assert_eq!(
        decode(&answer),
        Value::Int(7),
        "a Receiver reached with a detached Handle table must answer the same call the \
         dispatch handler would have made, so an implementation outside this crate is \
         unit-testable without a guest"
    );
}

// @behavior T-169
#[test]
fn a_byte_seam_receiver_sees_the_payload_the_guest_sent() {
    let Some(mut sandbox) = real_sandbox() else {
        return;
    };
    let echo = ByteEcho::new();
    sandbox
        .bind("Probe::Echo", echo.clone())
        .expect("bind the Receiver");

    let execution = sandbox
        .eval("Probe::Echo.call(7)")
        .expect("the invocation runs");

    assert_eq!(
        decode(execution.payload().expect("the guest returned")),
        Value::Int(7),
        "a Receiver answering on the byte seam must round-trip the dispatch through \
         the host's own encode"
    );
    let seen = echo.seen.lock().expect("never poisoned");
    assert_eq!(
        seen.len(),
        1,
        "the Receiver must be handed the dispatch exactly once"
    );
    assert_eq!(
        Arguments::decode(&seen[0]).expect("readable").args,
        vec![Value::Int(7)],
        "the bytes handed to the Receiver must be the Call's payload, undecoded by the SDK"
    );
}

/// A Receiver that yields on the byte seam, encoding the yield arguments
/// and decoding the block's answer itself.
struct ByteYielder;

impl Receiver for ByteYielder {
    fn call(
        &self,
        _method: &str,
        _payload: &[u8],
        block: Option<&mut Yielder<'_>>,
        _handles: &Handles<'_>,
    ) -> Result<Vec<u8>, Fault> {
        let block = block.ok_or_else(|| Fault::new(FaultKind::Argument, "a block is required"))?;
        let args = Encoder::encode(&Value::Array(vec![Value::Int(20)]))
            .map_err(|err| Fault::new(FaultKind::Runtime, format!("unencodable: {err}")))?;
        let answered = block
            .call_payload(&args)
            .map_err(|err| Fault::new(FaultKind::Runtime, format!("yield failed: {err}")))?;
        Ok(answered)
    }
}

/// A Receiver that propagates an unsupported yield, standing in for a
/// yield a guest did not serve: the bundled guest serves every block.
struct UnsupportedYield;

impl Receiver for UnsupportedYield {
    fn call(
        &self,
        _method: &str,
        _payload: &[u8],
        _block: Option<&mut Yielder<'_>>,
        _handles: &Handles<'_>,
    ) -> Result<Vec<u8>, Fault> {
        Err(kobako::YieldError::Unsupported(
            "the Sandbox runtime does not support yielding to a block".to_string(),
        )
        .into())
    }
}

// @behavior T-280
#[test]
fn an_unrescued_unsupported_yield_answers_in_kobakos_own_wording() {
    let Some(mut sandbox) = real_sandbox() else {
        return;
    };
    sandbox
        .bind("Probe::U", Arc::new(UnsupportedYield))
        .expect("bind the Receiver");

    let execution = sandbox
        .eval("Probe::U.call { |x| x }")
        .expect("the invocation runs");

    assert!(
        matches!(execution.payload(), Err(kobako::Error::Service(failure)) if !failure.message.contains("Kobako::")),
        "an unsupported yield a Receiver propagates through eval must fail the run as a \
         Service failure in kobako's own wording, not in the <class>: <message> shape a \
         Service's exception crosses in"
    );
}

// @behavior T-170
#[test]
fn call_payload_yields_with_host_encoded_bytes_and_returns_the_block_s_own() {
    let Some(mut sandbox) = real_sandbox() else {
        return;
    };
    sandbox
        .bind("Probe::Y", Arc::new(ByteYielder))
        .expect("bind the Receiver");

    let execution = sandbox
        .eval("Probe::Y.call { |n| n + 1 }")
        .expect("the invocation runs");

    assert_eq!(
        decode(execution.payload().expect("the guest returned")),
        Value::Int(21),
        "a yield driven on the byte seam must carry the host's encoded arguments into \
         the block and hand back the block's answer as bytes"
    );
}

// @behavior T-012
#[test]
fn a_built_run_payload_past_the_handle_ceiling_fails_the_run_before_the_guest_starts() {
    let Some(mut sandbox) = real_sandbox() else {
        return;
    };
    sandbox
        .preload("Echo", "class Echo; def self.call(value) = value; end")
        .expect("preload the entrypoint");
    let object: Arc<dyn Receiver> = ByteEcho::new();

    let result = sandbox.run(
        "Echo",
        RunPayload::from_fn(move |handles| {
            for _ in 0..=1_000_000 {
                handles.alloc(object.clone())?;
            }
            Ok(run_args(Value::Nil))
        }),
    );

    assert!(
        matches!(&result, Err(Error::Sandbox(failure)) if failure.name == "Kobako::HandleExhaustedError"),
        "a run payload built through RunPayload::from_fn that binds one host object more \
         than an invocation may hold must fail the run as the Sandbox failure \
         Kobako::HandleExhaustedError, reported with `?` alone"
    );
}

/// A host schema of its own for a result: a text value, a Handle spelled as
/// a bare integer id. Each failure it detects is reported through kobako's
/// own constructors, so its attribution matches the bundled codec's.
fn read_label(execution: &Execution) -> Result<String, Error> {
    let bytes = execution.payload().map_err(Clone::clone)?;
    match decode(bytes) {
        Value::Str(text) => Ok(text),
        Value::Int(id) => {
            let id = u32::try_from(id).map_err(Error::invalid_result)?;
            execution
                .resolve(id)
                .map(|_| "a live Handle".to_string())
                .ok_or_else(|| Error::unknown_handle(id))
        }
        other => Err(Error::invalid_result(format!(
            "expected text or a Handle id, found {other:?}"
        ))),
    }
}

// @behavior OC-003 OC-004 OC-005
#[test]
fn a_result_a_host_schema_cannot_read_fails_as_the_sandbox_with_its_detail_kept_aside() {
    let Some(sandbox) = real_sandbox() else {
        return;
    };
    let execution = sandbox.eval("[1, 2]").expect("the invocation runs");

    let Err(Error::Sandbox(failure)) = read_label(&execution) else {
        panic!(
            "a result a host's own schema cannot read must fail as a Sandbox failure \
             through Error::invalid_result"
        );
    };
    assert!(
        !failure.message.contains("expected text"),
        "the failure's message must name an invalid result and not the schema's detail"
    );
    assert!(
        failure
            .diagnostic
            .as_deref()
            .is_some_and(|detail| detail.contains("expected text")),
        "the schema's own account of the unreadable result must stay reachable as the \
         failure's diagnostic"
    );
}

// @behavior OC-053
#[test]
fn a_result_naming_a_handle_never_issued_fails_as_the_sandbox() {
    let Some(sandbox) = real_sandbox() else {
        return;
    };
    let execution = sandbox.eval("99").expect("the invocation runs");

    assert!(
        matches!(read_label(&execution), Err(Error::Sandbox(_))),
        "a result whose host schema names a Handle id the invocation never issued must \
         fail as a Sandbox failure through Error::unknown_handle"
    );
}
