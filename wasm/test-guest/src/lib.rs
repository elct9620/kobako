//! test-guest — minimal wasm fixture for host-side Sandbox#run E2E tests.
//!
//! Does NOT embed mruby; Frame 2 (user script) bytes from WASI stdin are
//! interpreted as a decimal integer (or special keywords) to drive outcome
//! branches. Frame 1 (preamble) is consumed and discarded — its shape is
//! validated only to confirm framing is intact.
//!
//! Stdin protocol (SPEC.md §ABI Signatures): two length-prefixed frames, each
//! headed by a 4-byte big-endian u32 length followed by that many payload
//! bytes. Frame 1 = preamble msgpack; Frame 2 = user script UTF-8.

#![allow(unsafe_code)]

use core::cell::RefCell;
use kobako_wasm::{
    decode_response, encode_outcome, encode_request, Decoder, Outcome, Panic, Request, Response,
    ResultEnv, Target, Value,
};

// Imported so the wasm import table contains `env::__kobako_rpc_call`
// (an exact-shape match of the production guest). Item #18 wires this to
// the host RPC dispatcher; this fixture invokes it from the `rpc:` source
// branch to exercise the host-side dispatch path end-to-end.
#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "env")]
extern "C" {
    fn __kobako_rpc_call(req_ptr: u32, req_len: u32) -> u64;
}

// ---------------------------------------------------------------------------
// Bump allocator backing __kobako_alloc.
// ---------------------------------------------------------------------------

const ARENA_SIZE: usize = 64 * 1024;

thread_local! {
    static ARENA: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    static ARENA_OFFSET: RefCell<usize> = const { RefCell::new(0) };
    static OUTCOME_BUFFER: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

fn ensure_arena() {
    ARENA.with(|a| {
        let mut a = a.borrow_mut();
        if a.is_empty() {
            a.resize(ARENA_SIZE, 0);
        }
    });
}

#[no_mangle]
pub extern "C" fn __kobako_alloc(size: u32) -> u32 {
    ensure_arena();
    let size = size as usize;
    let mut ptr = 0_u32;
    ARENA_OFFSET.with(|o| {
        let mut off = o.borrow_mut();
        if *off + size > ARENA_SIZE {
            ptr = 0;
            return;
        }
        ARENA.with(|a| {
            let a = a.borrow();
            // Compute the linear-memory address by reading the slice's
            // start. wasm32 lets us cast `&u8` to a u32 address.
            let base = a.as_ptr() as u32;
            ptr = base + (*off as u32);
        });
        *off += size;
    });
    ptr
}

// ---------------------------------------------------------------------------
// __kobako_run — read stdin two-frame protocol, build outcome from Frame 2.
// ---------------------------------------------------------------------------

/// SPEC ABI `() -> ()` shape. Source arrives via WASI stdin two-frame
/// protocol (SPEC.md §ABI Signatures): Frame 1 (preamble msgpack) is read
/// and discarded; Frame 2 (user script UTF-8) drives the outcome.
#[no_mangle]
pub extern "C" fn __kobako_run() {
    // Write a fixed marker to stdout so the host can assert non-empty capture
    // (SPEC.md §B-04 — stdout/stderr capture E2E verification).
    println!("hello from test-guest");

    // Read both frames from stdin. Frame 1 is consumed and discarded;
    // Frame 2 is the source bytes this fixture acts on.
    let _frame1 = read_stdin_frame().unwrap_or_default();
    let source = read_stdin_frame().unwrap_or_default();

    let outcome = build_outcome(&source);
    let bytes = encode_outcome(&outcome).expect("encode outcome");

    OUTCOME_BUFFER.with(|b| {
        let mut buf = b.borrow_mut();
        *buf = bytes;
    });
}

/// Read one length-prefixed frame from WASI stdin. Frame format:
/// 4-byte big-endian u32 length prefix + that many payload bytes.
/// Returns `None` on EOF or read error.
#[cfg(target_arch = "wasm32")]
fn read_stdin_frame() -> Option<Vec<u8>> {
    use std::io::Read;
    let mut len_buf = [0u8; 4];
    std::io::stdin().read_exact(&mut len_buf).ok()?;
    let len = u32::from_be_bytes(len_buf) as usize;
    let mut payload = vec![0u8; len];
    std::io::stdin().read_exact(&mut payload).ok()?;
    Some(payload)
}

#[cfg(not(target_arch = "wasm32"))]
fn read_stdin_frame() -> Option<Vec<u8>> {
    // On the host (cargo test) there is no WASI stdin; return an empty
    // payload so unit tests that call this path get a deterministic result.
    Some(Vec::new())
}

fn build_outcome(source: &[u8]) -> Outcome {
    let text = match core::str::from_utf8(source) {
        Ok(s) => s.trim(),
        Err(_) => "",
    };

    if text == "panic" {
        return Outcome::Panic(Panic {
            origin: "sandbox".into(),
            class: "RuntimeError".into(),
            message: "boom".into(),
            backtrace: vec!["test-guest:1".into()],
            details: None,
        });
    }

    // `panic:service` — emit a Panic envelope with origin="service" so the
    // host's two-step error attribution maps the outcome to
    // `Kobako::ServiceError`.
    if text == "panic:service" {
        return Outcome::Panic(Panic {
            origin: "service".into(),
            class: "Kobako::ServiceError".into(),
            message: "service exploded".into(),
            backtrace: vec!["test-guest:1".into()],
            details: None,
        });
    }

    // `trap` — execute the wasm `unreachable` instruction. Wasmtime sees a
    // native trap and the host attributes the run to `Kobako::TrapError`
    // (SPEC §"Step 1 — Wasm trap").
    if text == "trap" {
        #[cfg(target_arch = "wasm32")]
        {
            core::arch::wasm32::unreachable();
        }
        #[cfg(not(target_arch = "wasm32"))]
        unreachable!();
    }

    // `rpc-panic:Group::Member|method|argument` — same RPC round-trip as
    // `rpc:` but the err branch is reified as a Panic(origin=service)
    // envelope. Exercises the wire integration path that surfaces a
    // Service exception to the host as `Kobako::ServiceError`. SPEC §B-12
    // / §"Error Scenarios" E-11.
    if let Some(rest) = text.strip_prefix("rpc-panic:") {
        return run_rpc_panic(rest);
    }

    // `rpc-chain:Group::Factory|factory_method|factory_arg|target_method`
    // — two-step Handle chaining (B-17). First call returns a Handle; the
    // second call uses that Handle as the Request target.
    if let Some(rest) = text.strip_prefix("rpc-chain:") {
        return run_rpc_chain(rest);
    }

    // `rpc-kwargs:Group::Member|method|k1=v1,k2=v2` — RPC with kwargs.
    // Exercises the kwargs symbolize-at-boundary path (E-15).
    if let Some(rest) = text.strip_prefix("rpc-kwargs:") {
        return run_rpc_kwargs(rest);
    }

    // `rpc-dc-chain:Group::Setup|setup_method|target_method` — exercises
    // the disconnected-sentinel path (E-14). The setup method must:
    //   1. allocate an object in the HandleTable,
    //   2. mark it disconnected, and
    //   3. return the integer id.
    // The fixture then issues a second RPC with `Target::Handle(id)`,
    // observes a `type="disconnected"` Response.err, and emits a
    // Panic(origin=service, class="Kobako::ServiceError::Disconnected").
    if let Some(rest) = text.strip_prefix("rpc-dc-chain:") {
        return run_rpc_dc_chain(rest);
    }

    // `rpc:Group::Member:method:argument` — exercise the host-side RPC
    // dispatch path (item #18). Build a Request envelope, hand it to
    // `__kobako_rpc_call`, decode the Response, and embed the outcome
    // value (or an "err:<type>" sentinel) in a Result envelope so the
    // host test can assert the round-trip.
    if let Some(rest) = text.strip_prefix("rpc:") {
        return run_rpc(rest);
    }

    // `handle:N` — emit a Result envelope carrying ext 0x01 Handle(N).
    // Used by host tests to stage a Handle id whose validity must NOT
    // survive into the next #run (cross-run Handle invalidity).
    if let Some(rest) = text.strip_prefix("handle:") {
        if let Ok(id) = rest.parse::<u32>() {
            return Outcome::Result(ResultEnv {
                value: Value::Handle(id),
            });
        }
    }

    // Default: parse as i64 and wrap in a Result envelope.
    match text.parse::<i64>() {
        Ok(n) => Outcome::Result(ResultEnv {
            value: Value::Int(n),
        }),
        Err(_) => Outcome::Result(ResultEnv {
            value: Value::Str(text.to_string()),
        }),
    }
}

// ---------------------------------------------------------------------------
// rpc:Group::Member:method:argument — drive the host RPC dispatch path.
// ---------------------------------------------------------------------------
//
// Source format (deliberately tiny — this fixture is a string-driven
// trigger, not an mruby host). Fields separated by `|` to avoid
// ambiguity with the `::` namespace separator inside the target path:
//
//     rpc:<Group::Member>|<method>|<argument>
//
// Builds a Request envelope `[Path("Group::Member"), method, [argument], {}]`,
// invokes `__kobako_rpc_call`, decodes the Response, and wraps the outcome
// in a Result envelope so the host test can assert the round-trip.
//
//   * Response.ok(Value::Str(s))   → Result(Value::Str(s))
//   * Response.ok(Value::Int(n))   → Result(Value::Int(n))
//   * Response.ok(<other>)         → Result(Str("ok:<other>"))
//   * Response.err(<exception>)    → Result(Str("err:<type>"))
//
// Returns a Panic envelope when the source format is malformed or when
// the host import returns a zero packed value (wire-layer failure).
fn run_rpc(rest: &str) -> Outcome {
    let parts: Vec<&str> = rest.splitn(3, '|').collect();
    if parts.len() != 3 {
        return malformed_rpc("expected rpc:Group::Member|method|argument");
    }
    let target = parts[0];
    let method = parts[1];
    let argument = parts[2];

    let req = Request {
        target: Target::Path(target.to_string()),
        method: method.to_string(),
        args: vec![Value::Str(argument.to_string())],
        kwargs: vec![],
    };
    let req_bytes = match encode_request(&req) {
        Ok(b) => b,
        Err(_) => return malformed_rpc("encode_request failed"),
    };

    let resp_bytes = match invoke_rpc_import(&req_bytes) {
        Some(b) => b,
        None => return malformed_rpc("rpc import returned 0 (wire fault)"),
    };

    let response = match decode_response(&resp_bytes) {
        Ok(r) => r,
        Err(_) => return malformed_rpc("decode_response failed"),
    };

    match response {
        Response::Ok(Value::Str(s)) => Outcome::Result(ResultEnv {
            value: Value::Str(s),
        }),
        Response::Ok(Value::Int(n)) => Outcome::Result(ResultEnv {
            value: Value::Int(n),
        }),
        Response::Ok(Value::Nil) => Outcome::Result(ResultEnv { value: Value::Nil }),
        Response::Ok(other) => Outcome::Result(ResultEnv {
            value: Value::Str(format!("ok:{:?}", other)),
        }),
        Response::Err(payload) => {
            // Best-effort surface of the exception type. The payload is
            // a msgpack map carried opaquely by the codec; rather than
            // re-decode it, we return a fixed marker so the host test
            // can assert the err-branch was taken.
            Outcome::Result(ResultEnv {
                value: Value::Str(format!("err:{}bytes", payload.len())),
            })
        }
    }
}

fn malformed_rpc(msg: &str) -> Outcome {
    Outcome::Panic(Panic {
        origin: "sandbox".into(),
        class: "RuntimeError".into(),
        message: msg.into(),
        backtrace: vec!["test-guest:rpc".into()],
        details: None,
    })
}

/// Invoke the `__kobako_rpc_call` host import and read the response
/// bytes back from guest linear memory. Returns `None` when the host
/// returns 0 (reserved for wire-layer faults).
fn invoke_rpc_import(req_bytes: &[u8]) -> Option<Vec<u8>> {
    #[cfg(target_arch = "wasm32")]
    unsafe {
        let req_ptr = req_bytes.as_ptr() as u32;
        let req_len = req_bytes.len() as u32;
        let packed = __kobako_rpc_call(req_ptr, req_len);
        if packed == 0 {
            return None;
        }
        let resp_ptr = (packed >> 32) as u32;
        let resp_len = (packed & 0xffff_ffff) as u32;
        let slice = core::slice::from_raw_parts(resp_ptr as *const u8, resp_len as usize);
        Some(slice.to_vec())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = req_bytes;
        None
    }
}

// ---------------------------------------------------------------------------
// rpc-panic / rpc-chain / rpc-kwargs / rpc-dc-chain — wire-integration
// fixture paths used by `test/test_wire_integration.rb`. Each builds one
// or more Request envelopes via `__kobako_rpc_call` and reifies the host
// response into the appropriate Outcome envelope so the host can assert
// the full round-trip semantics through `Sandbox#run`.
// ---------------------------------------------------------------------------

// `rpc-panic:Group::Member|method|argument` — drive the RPC path and
// surface the err branch as a Panic(origin=service) outcome. Exercises
// the SPEC E-11 attribution path (Service exception → ServiceError).
fn run_rpc_panic(rest: &str) -> Outcome {
    let parts: Vec<&str> = rest.splitn(3, '|').collect();
    if parts.len() != 3 {
        return malformed_rpc("expected rpc-panic:Group::Member|method|argument");
    }
    let req = Request {
        target: Target::Path(parts[0].to_string()),
        method: parts[1].to_string(),
        args: vec![Value::Str(parts[2].to_string())],
        kwargs: vec![],
    };
    match dispatch_request(&req) {
        Ok(Response::Ok(value)) => Outcome::Result(ResultEnv { value }),
        Ok(Response::Err(payload)) => panic_from_err_payload(&payload),
        Err(msg) => malformed_rpc(msg),
    }
}

// `rpc-chain:Group::Factory|factory_method|factory_arg|target_method` —
// exercises B-17 Handle target dispatch. First RPC's response must be
// Value::Handle(id); the fixture re-uses that id as the Target of a
// second RPC, returning the second call's value as the outcome.
fn run_rpc_chain(rest: &str) -> Outcome {
    let parts: Vec<&str> = rest.splitn(4, '|').collect();
    if parts.len() != 4 {
        return malformed_rpc(
            "expected rpc-chain:Group::Factory|factory_method|factory_arg|target_method",
        );
    }
    let factory_req = Request {
        target: Target::Path(parts[0].to_string()),
        method: parts[1].to_string(),
        args: vec![Value::Str(parts[2].to_string())],
        kwargs: vec![],
    };
    let handle_id = match dispatch_request(&factory_req) {
        Ok(Response::Ok(Value::Handle(id))) => id,
        Ok(Response::Ok(other)) => {
            return malformed_rpc_owned(format!("chain factory returned non-Handle: {:?}", other));
        }
        Ok(Response::Err(_)) => return malformed_rpc("chain factory returned err"),
        Err(msg) => return malformed_rpc(msg),
    };
    let target_req = Request {
        target: Target::Handle(handle_id),
        method: parts[3].to_string(),
        args: vec![],
        kwargs: vec![],
    };
    match dispatch_request(&target_req) {
        Ok(Response::Ok(value)) => Outcome::Result(ResultEnv { value }),
        Ok(Response::Err(payload)) => panic_from_err_payload(&payload),
        Err(msg) => malformed_rpc(msg),
    }
}

// `rpc-kwargs:Group::Member|method|k1=v1,k2=v2` — RPC with string-keyed
// kwargs. The Registry symbolizes the keys at the boundary (E-15) and
// the bound Service Member receives Symbol-keyed kwargs.
fn run_rpc_kwargs(rest: &str) -> Outcome {
    let parts: Vec<&str> = rest.splitn(3, '|').collect();
    if parts.len() != 3 {
        return malformed_rpc("expected rpc-kwargs:Group::Member|method|k1=v1,k2=v2");
    }
    let kwargs = match parse_kwargs(parts[2]) {
        Ok(kw) => kw,
        Err(msg) => return malformed_rpc(msg),
    };
    let req = Request {
        target: Target::Path(parts[0].to_string()),
        method: parts[1].to_string(),
        args: vec![],
        kwargs,
    };
    match dispatch_request(&req) {
        Ok(Response::Ok(value)) => Outcome::Result(ResultEnv { value }),
        Ok(Response::Err(payload)) => panic_from_err_payload(&payload),
        Err(msg) => malformed_rpc(msg),
    }
}

// `rpc-dc-chain:Group::Setup|setup_method|target_method` — exercises the
// E-14 disconnected-sentinel path. The Setup service is bound by the
// host test such that calling it allocates a HandleTable entry, marks
// it `:disconnected`, and returns the integer id. The fixture then uses
// `Target::Handle(id)` for the second RPC; the host returns a
// `type="disconnected"` Response.err which the fixture surfaces as a
// Panic(origin=service, class="Kobako::ServiceError::Disconnected").
fn run_rpc_dc_chain(rest: &str) -> Outcome {
    let parts: Vec<&str> = rest.splitn(3, '|').collect();
    if parts.len() != 3 {
        return malformed_rpc("expected rpc-dc-chain:Group::Setup|setup_method|target_method");
    }
    let setup_req = Request {
        target: Target::Path(parts[0].to_string()),
        method: parts[1].to_string(),
        args: vec![],
        kwargs: vec![],
    };
    let handle_id = match dispatch_request(&setup_req) {
        Ok(Response::Ok(Value::Int(id))) if id >= 0 => id as u32,
        Ok(Response::Ok(Value::UInt(id))) => id as u32,
        Ok(Response::Ok(other)) => {
            return malformed_rpc_owned(format!("dc-chain setup returned non-int: {:?}", other));
        }
        Ok(Response::Err(_)) => return malformed_rpc("dc-chain setup returned err"),
        Err(msg) => return malformed_rpc(msg),
    };
    let target_req = Request {
        target: Target::Handle(handle_id),
        method: parts[2].to_string(),
        args: vec![],
        kwargs: vec![],
    };
    match dispatch_request(&target_req) {
        Ok(Response::Ok(value)) => Outcome::Result(ResultEnv { value }),
        Ok(Response::Err(payload)) => panic_from_err_payload(&payload),
        Err(msg) => malformed_rpc(msg),
    }
}

// Encode the Request, invoke the host import, decode the Response.
// Returns Err with a static reason on encode/decode/transport faults.
fn dispatch_request(req: &Request) -> Result<Response, &'static str> {
    let req_bytes = encode_request(req).map_err(|_| "encode_request failed")?;
    let resp_bytes = invoke_rpc_import(&req_bytes).ok_or("rpc import returned 0 (wire fault)")?;
    decode_response(&resp_bytes).map_err(|_| "decode_response failed")
}

// Decode the inner ext 0x02 Exception map from a Response.err payload
// and synthesise a Panic(origin=service) envelope. The panic class
// field follows SPEC §"Error Class Hierarchy":
//   * type=="disconnected" → "Kobako::ServiceError::Disconnected"
//   * everything else      → "Kobako::ServiceError"
// so the host's `Sandbox#build_panic_error` selects the right Ruby class.
fn panic_from_err_payload(payload: &[u8]) -> Outcome {
    let (typ, message) = decode_exception_payload(payload)
        .unwrap_or_else(|| ("runtime".into(), "<undecodable>".into()));
    let class = if typ == "disconnected" {
        "Kobako::ServiceError::Disconnected".to_string()
    } else {
        "Kobako::ServiceError".to_string()
    };
    Outcome::Panic(Panic {
        origin: "service".into(),
        class,
        message,
        backtrace: vec!["test-guest:rpc-panic".into()],
        details: None,
    })
}

// Decode the ext 0x02 inner map's `type` and `message` string fields.
// Returns None if the payload is not a map or the keys are absent /
// non-string. Forward-compatible: unknown keys are silently ignored.
fn decode_exception_payload(payload: &[u8]) -> Option<(String, String)> {
    let mut dec = Decoder::new(payload);
    let value = dec.read_value().ok()?;
    let pairs = match value {
        Value::Map(p) => p,
        _ => return None,
    };
    let mut typ = None;
    let mut msg = None;
    for (k, v) in pairs {
        if let (Value::Str(name), Value::Str(s)) = (&k, &v) {
            match name.as_str() {
                "type" => typ = Some(s.clone()),
                "message" => msg = Some(s.clone()),
                _ => {}
            }
        }
    }
    Some((typ?, msg?))
}

// Parse a comma-separated `k=v,k2=v2` kwargs spec into the Vec shape
// expected by `Request.kwargs`. Empty input means no kwargs.
fn parse_kwargs(spec: &str) -> Result<Vec<(String, Value)>, &'static str> {
    if spec.is_empty() {
        return Ok(vec![]);
    }
    let mut pairs = Vec::new();
    for entry in spec.split(',') {
        let mut kv = entry.splitn(2, '=');
        let key = kv.next().ok_or("kwargs: missing key")?;
        let value = kv.next().ok_or("kwargs: missing value")?;
        pairs.push((key.to_string(), Value::Str(value.to_string())));
    }
    Ok(pairs)
}

fn malformed_rpc_owned(msg: String) -> Outcome {
    Outcome::Panic(Panic {
        origin: "sandbox".into(),
        class: "RuntimeError".into(),
        message: msg,
        backtrace: vec!["test-guest:rpc".into()],
        details: None,
    })
}

// ---------------------------------------------------------------------------
// __kobako_take_outcome — returns packed (ptr, len) of OUTCOME_BUFFER.
// ---------------------------------------------------------------------------

#[no_mangle]
pub extern "C" fn __kobako_take_outcome() -> u64 {
    let mut packed = 0_u64;
    OUTCOME_BUFFER.with(|b| {
        let buf = b.borrow();
        let ptr = buf.as_ptr() as u32;
        let len = buf.len() as u32;
        packed = ((ptr as u64) << 32) | (len as u64);
    });
    packed
}
