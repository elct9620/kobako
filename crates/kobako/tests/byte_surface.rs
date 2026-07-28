//! Integration coverage for the byte-level payload surface — the entries
//! a host uses when the payload's schema is its own: `Sandbox::run_payload`,
//! `Execution::payload`, `Receiver::call`, and `Yielder::call_payload`.
//!
//! The bundled guest speaks MessagePack, so these tests build that schema's
//! bytes themselves rather than reaching for the `Value` shortcuts. That is
//! the point: what the byte surface promises is that the host owns the
//! encode and decode, and the bytes it hands over ride verbatim. A host
//! whose guest speaks another schema substitutes its own encoder at exactly
//! these call sites.
//!
//! Driven through the real guest binary; a missing binary is a hard failure
//! under CI and a silent skip locally, mirroring the Ruby E2E helper.

use std::path::Path;
use std::sync::{Arc, Mutex};

use kobako::{Fault, FaultKind, Handles, Options, Receiver, Sandbox, Yielder};
use kobako_codec::msgpack::codec::{Decode, Decoder, Encode, Encoder, Value};
use kobako_codec::msgpack::payload::Arguments;

const WASM: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/kobako.wasm");

fn real_sandbox() -> Option<Sandbox> {
    if !Path::new(WASM).exists() {
        assert!(
            std::env::var_os("CI").is_none(),
            "data/kobako.wasm missing under CI — run `bundle exec rake wasm:build`"
        );
        return None;
    }
    Some(Sandbox::new(WASM, Options::default()).expect("construct the Sandbox"))
}

/// The host's own encode of a `run` payload — one positional argument.
fn run_args(value: Value) -> Vec<u8> {
    Arguments::new(vec![value], Vec::new())
        .encode()
        .expect("a single-argument payload always encodes")
}

fn decode(bytes: &[u8]) -> Value {
    Decoder::new(bytes)
        .read_only_value()
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

#[test]
fn run_payload_carries_the_host_s_own_bytes_and_payload_hands_them_back() {
    let Some(mut sandbox) = real_sandbox() else {
        return;
    };
    sandbox
        .preload("Echo", "class Echo; def self.call(value) = value; end")
        .expect("preload the entrypoint");

    let execution = sandbox
        .run_payload("Echo", |_handles| Ok(run_args(Value::Str("hi".into()))))
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
