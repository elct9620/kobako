//! Assemble a kobako host by hand — no SDK.
//!
//! The `kobako` SDK crate wraps all of this behind `Sandbox`; here the
//! wire is exposed on purpose. This is the seam the SDK is built on, and
//! the reference a non-Rust frontend author follows to drive the same
//! SPEC wire in another language. Four published crates are the whole
//! toolkit, and the split between the last two is the point:
//!
//!   * `kobako-wasmtime` gives the `Driver` that runs a prebuilt Guest
//!     Binary on a fresh instance per invocation;
//!   * `kobako-runtime` is the engine-neutral contract the driver
//!     implements — `Runtime`, `Snapshot`, the dispatch traits;
//!   * `kobako-transport` owns the core envelope — the frames, the Call
//!     and its Reply, the Outcome — every kobako assembly shares;
//!   * `kobako-codec` owns only what rides *inside* an envelope, the
//!     payload, under the schema this host happens to speak.
//!
//! The host drives one `#eval`-equivalent invocation. Frame 1 registers
//! a `MyService::KV` constant the guest reaches like any other, and a
//! hand-written `DispatchHandler` answers every Call the guest makes
//! against it: read the routing fields the runtime already decoded,
//! route to an in-process store, answer with a `Reply`. The handler
//! honours the one hard rule of the dispatch contract — it never fails,
//! folding every error into the Reply's fault arm, which the guest
//! re-raises as a rescuable exception rather than a wasm trap.
//!
//! A Fault is typed on the envelope rather than encoded in the payload,
//! so refusing a call reaches no codec at all — the half of this example
//! a host speaking another schema would keep unchanged.

use std::collections::HashMap;
use std::env;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use kobako_codec::msgpack::{Arguments, Decode, Decoder, Encoder, Value};
use kobako_runtime::{
    Capture, Completion, DispatchHandler, Entry, Frames, InvokeError, Profile, Runtime, SetupError,
    Snapshot, Trap, Yielder,
};
use kobako_transport::envelope::{
    Bindings, Call, Fault, FaultKind, Outcome, Panic, Reply, Snippets, Target,
};
use kobako_wasmtime::{Config, Driver};

/// Demo source when none is given on the command line: a round-trip
/// through the store, a rescued Service fault, and a miss returning
/// `nil` — the three answers a dispatch can come back with.
const DEFAULT_SOURCE: &str = r##"
MyService::KV.set("greeting", "hello via dispatch")
puts MyService::KV.get("greeting")

begin
  MyService::KV.remove("greeting")
rescue => e
  puts "rescued: #{e.class}: #{e.message}"
end

MyService::KV.get("missing")
"##;

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let Some(wasm_path) = args.next().map(PathBuf::from) else {
        eprintln!("usage: kobako-wire-host <path/to/kobako.wasm> [mruby-source]");
        return ExitCode::FAILURE;
    };
    let source = args
        .next()
        .unwrap_or_else(|| DEFAULT_SOURCE.trim().to_string());

    // The caps a Ruby host passes as Sandbox options; `Hermetic` is the
    // full ambient-denial posture — frozen clocks and entropy.
    let config = Config {
        timeout: Some(Duration::from_secs(5)),
        memory_limit: Some(64 * 1024 * 1024),
        stdout_limit: Some(64 * 1024),
        stderr_limit: Some(64 * 1024),
        profile: Profile::Hermetic,
    };
    let driver = match Driver::new(&wasm_path, config) {
        Ok(driver) => driver,
        Err(setup) => {
            report_setup_error(&setup);
            return ExitCode::FAILURE;
        }
    };

    // Frame 1 carries the registration preamble: the bound constant
    // paths. The guest installs a proxy constant for each, so guest code
    // reaches the store as plain `MyService::KV` calls. Frame 3
    // (preloaded snippets) is mandatory-presence too: this host preloads
    // nothing, an empty list rather than an absent frame.
    //
    // Both frames are envelope types, so neither reaches a payload
    // codec — a host on another schema writes these two lines unchanged.
    let preamble = Bindings {
        paths: vec!["MyService::KV".to_string()],
    }
    .encode();
    let snippets = Snippets::default().encode();
    let handler = Arc::new(KvHandler::default());
    let snapshot = driver.invoke(
        Entry::Eval {
            source: source.as_bytes(),
        },
        Frames {
            preamble: &preamble,
            snippets: &snippets,
        },
        Some(handler.clone()),
    );

    let exit = match snapshot {
        Ok(snapshot) => report_snapshot(&snapshot),
        Err(InvokeError::Setup(setup)) => {
            report_setup_error(&setup);
            ExitCode::FAILURE
        }
        Err(InvokeError::Trap(trap)) => {
            report_trap(&trap);
            ExitCode::FAILURE
        }
        // The channel set is open, so a later way to fail before the
        // guest starts still reports rather than failing to compile.
        Err(other) => {
            eprintln!("invocation never started: {other}");
            ExitCode::FAILURE
        }
    };

    // The store outlives the invocation on the host side — the state the
    // guest mutated through dispatch is ordinary host state now.
    for (key, value) in handler.entries() {
        println!("host store: {key:?} => {}", render(&value));
    }
    exit
}

/// An in-process key-value store exposed to the guest as `MyService::KV`
/// — the host side of every dispatch the demo source makes.
#[derive(Default)]
struct KvHandler {
    store: Mutex<HashMap<String, Value>>,
}

impl KvHandler {
    /// Snapshot of the store for the post-invocation readout.
    fn entries(&self) -> Vec<(String, Value)> {
        let store = self.lock_store();
        let mut entries: Vec<(String, Value)> =
            store.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        entries.sort_by(|(a, _), (b, _)| a.cmp(b));
        entries
    }

    /// Route one Call to the store, mirroring the fault taxonomy the Ruby
    /// dispatcher uses: `Undefined` for an unknown target or method,
    /// `Argument` for a call shape the method does not take.
    ///
    /// The routing fields arrive already decoded — reading the target and
    /// method costs no codec, which is why the payload is not touched
    /// until a method has been chosen.
    fn handle(&self, call: &Call<'_>) -> Reply {
        let Target::Path(path) = call.target else {
            return fault(
                FaultKind::Undefined,
                "this host allocates no Capability Handles",
            );
        };
        if path != "MyService::KV" {
            return fault(FaultKind::Undefined, format!("unknown constant {path}"));
        }
        let arguments = match Arguments::decode(call.payload) {
            Ok(arguments) => arguments,
            Err(err) => {
                return fault(
                    FaultKind::Runtime,
                    format!("Sandbox received a malformed payload: {err}"),
                )
            }
        };
        if !arguments.kwargs.is_empty() {
            return fault(
                FaultKind::Argument,
                "KV methods accept no keyword arguments",
            );
        }
        match (call.method, arguments.args.as_slice()) {
            ("get", [Value::Str(key)]) => {
                let value = self.lock_store().get(key).cloned().unwrap_or(Value::Nil);
                ok(&value)
            }
            ("set", [Value::Str(key), value]) => {
                self.lock_store().insert(key.clone(), value.clone());
                ok(value)
            }
            ("get" | "set", _) => fault(
                FaultKind::Argument,
                "get takes one String key; set takes a String key and a value",
            ),
            (method, _) => fault(
                FaultKind::Undefined,
                format!("method :{method} is not a Service method"),
            ),
        }
    }

    /// A poisoned lock only means a previous holder panicked; the map
    /// itself is still usable, and the never-fail dispatch contract
    /// outranks poisoning hygiene here.
    fn lock_store(&self) -> std::sync::MutexGuard<'_, HashMap<String, Value>> {
        self.store
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl DispatchHandler for KvHandler {
    /// `None` is reserved for "the handler itself failed"; this handler
    /// reifies every failure as a fault Reply instead, so the guest always
    /// receives an answer.
    ///
    /// The envelope is already decoded on the way in and encoded on the
    /// way out, so there is no longer a malformed-Call arm here — a Call
    /// the runtime could not read never reaches a handler.
    fn dispatch(&self, call: Call<'_>, _yielder: &mut dyn Yielder) -> Option<Reply> {
        Some(self.handle(&call))
    }
}

/// A successful Reply carrying one value under this host's payload
/// schema. A value the wire cannot carry back — nested past the depth cap
/// — folds into a fault like every other failure.
fn ok(value: &Value) -> Reply {
    match Encoder::encode(value) {
        Ok(bytes) => Reply::Ok(bytes),
        Err(err) => fault(FaultKind::Runtime, format!("value not encodable: {err}")),
    }
}

/// A refusal. Typed on the envelope, so the category and the message are
/// the whole of it and no payload codec is involved.
fn fault(kind: FaultKind, message: impl Into<String>) -> Reply {
    Reply::Fault(Fault::new(kind, message))
}

/// Print every observable of a completed invocation — captures, usage,
/// then the decoded completion — and translate each completion / failure
/// channel into an exit code, the way a frontend maps `Outcome`, `Trap`,
/// and `SetupError` onto its own error surface.
fn report_snapshot(snapshot: &Snapshot) -> ExitCode {
    report_capture("stdout", &snapshot.stdout);
    report_capture("stderr", &snapshot.stderr);
    println!(
        "usage: wall_time={:.6}s memory_peak={} bytes",
        snapshot.usage.wall_time, snapshot.usage.memory_peak
    );

    match &snapshot.completion {
        // The envelope frames the arm; only the ok arm's body is this
        // host's schema to read.
        Completion::Outcome(bytes) => match Outcome::decode(bytes) {
            Ok(Outcome::Ok(body)) => match Decoder::new(&body).read_only_value() {
                Ok(value) => {
                    println!("=> {}", render(&value));
                    ExitCode::SUCCESS
                }
                Err(err) => {
                    eprintln!("outcome body this schema cannot read: {err}");
                    ExitCode::FAILURE
                }
            },
            Ok(Outcome::Panic(panic)) => {
                report_panic(&panic);
                ExitCode::FAILURE
            }
            Err(err) => {
                eprintln!("malformed outcome bytes: {err}");
                ExitCode::FAILURE
            }
        },
        Completion::Trap(trap) => {
            report_trap(trap);
            ExitCode::FAILURE
        }
    }
}

fn report_capture(name: &str, capture: &Capture) {
    if capture.bytes.is_empty() {
        return;
    }
    let clipped = if capture.truncated {
        " (truncated)"
    } else {
        ""
    };
    println!("{name}{clipped}:");
    print!("{}", String::from_utf8_lossy(&capture.bytes));
}

/// A guest-side failure: the script raised (or was rejected) but the
/// invocation itself completed — the Ruby gem maps this to
/// `Kobako::SandboxError`.
fn report_panic(panic: &Panic) {
    eprintln!(
        "guest panic [{}] {}: {}",
        panic.origin.name(),
        panic.error.name,
        panic.error.message
    );
    for line in &panic.error.backtrace {
        eprintln!("  {line}");
    }
}

/// An engine fault after the export started — wall-clock cap,
/// linear-memory cap, or any other wasm trap. Captures and usage above
/// survive it.
fn report_trap(trap: &Trap) {
    // The kind set is open: a cap this frontend has no wording for is
    // still an engine fault, so it takes the base arm rather than
    // another cap's.
    let kind = match trap {
        Trap::Timeout(_) => "timeout",
        Trap::MemoryLimit(_) => "memory limit",
        _ => "trap",
    };
    eprintln!("guest {kind}: {trap}");
}

/// A failure that produced no invocation: the artifact is absent or
/// unusable (`ModuleNotBuilt` / `Dead`) or a host-side pre-call step
/// failed with the runtime still live (`Intact`).
fn report_setup_error(setup: &SetupError) {
    // Open for the same reason `Trap` is; an unrecognised state is still
    // a setup failure.
    let kind = match setup {
        SetupError::ModuleNotBuilt(_) => "guest artifact not built",
        SetupError::Dead(_) => "runtime dead",
        _ => "setup failed",
    };
    eprintln!("{kind}: {setup}");
}

/// Render a decoded wire `Value` in Ruby `#inspect` style, so the
/// printed result reads like what the guest script returned.
fn render(value: &Value) -> String {
    match value {
        Value::Nil => "nil".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Int(n) => n.to_string(),
        Value::UInt(n) => n.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Str(s) => format!("{s:?}"),
        Value::Bin(bytes) => format!("<{} binary bytes>", bytes.len()),
        Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(render).collect();
            format!("[{}]", inner.join(", "))
        }
        Value::Map(pairs) => {
            let inner: Vec<String> = pairs
                .iter()
                .map(|(k, v)| format!("{} => {}", render(k), render(v)))
                .collect();
            format!("{{{}}}", inner.join(", "))
        }
        Value::Sym(name) => format!(":{name}"),
        Value::Handle(id) => format!("#<Kobako::Handle {id}>"),
    }
}
