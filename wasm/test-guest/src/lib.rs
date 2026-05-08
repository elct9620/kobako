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
    decode_response, encode_outcome, encode_request, Outcome, Panic, Request, Response, ResultEnv,
    Target, Value,
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
