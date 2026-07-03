//! Minimal eval-only kobako host.
//!
//! The smallest complete host a Rust embedder can assemble from the
//! published crates: build a `Driver` over a prebuilt Guest Binary,
//! run one mruby source string on a fresh instance, and read every
//! observable back — the decoded return value or `Panic`, both capture
//! channels, and the resource usage. The Ruby gem's `Kobako::Sandbox`
//! is this same assembly plus the Service/Handle conveniences.

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use kobako_codec::codec::{Decode, Encoder, Value};
use kobako_codec::outcome::{Outcome, Panic};
use kobako_runtime::error::{Error, SetupError, Trap};
use kobako_runtime::profile::Profile;
use kobako_runtime::runtime::{Entry, Frames, Runtime};
use kobako_runtime::snapshot::{Capture, Completion, Snapshot};
use kobako_wasmtime::{Config, Driver};

/// Demo source when none is given on the command line: exercises both
/// capture (stdout) and a structured return value.
const DEFAULT_SOURCE: &str = r#"
puts "hello from mruby inside wasm"
{ squares: [1, 2, 3].map { |n| n * n }, engine: RUBY_ENGINE }
"#;

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let Some(wasm_path) = args.next().map(PathBuf::from) else {
        eprintln!("usage: minimal <path/to/kobako.wasm> [mruby-source]");
        return ExitCode::FAILURE;
    };
    let source = args
        .next()
        .unwrap_or_else(|| DEFAULT_SOURCE.trim().to_string());

    // The caps a Ruby host passes as Sandbox options; `Hermetic` is the
    // full ambient-denial posture (frozen clocks and entropy).
    let config = Config {
        timeout: Some(Duration::from_secs(5)),
        stdout_limit_bytes: Some(64 * 1024),
        stderr_limit_bytes: Some(64 * 1024),
        profile: Profile::Hermetic,
    };
    let driver = match Driver::new(&wasm_path, Some(64 * 1024 * 1024), config) {
        Ok(driver) => driver,
        Err(setup) => {
            report_setup_error(&setup);
            return ExitCode::FAILURE;
        }
    };

    // Frame 1 (Service registrations) and Frame 3 (preloaded snippets)
    // are mandatory-presence: an empty registry is an empty msgpack
    // array, never an absent frame. This host registers nothing.
    let preamble = empty_frame();
    let snippets = empty_frame();
    let snapshot = driver.invoke(
        Entry::Eval {
            source: source.as_bytes(),
        },
        Frames {
            preamble: &preamble,
            snippets: &snippets,
        },
        None,
    );

    match snapshot {
        Ok(snapshot) => report_snapshot(&snapshot),
        Err(Error::Setup(setup)) => {
            report_setup_error(&setup);
            ExitCode::FAILURE
        }
        Err(Error::Trap(trap)) => {
            report_trap(&trap);
            ExitCode::FAILURE
        }
    }
}

/// Print every observable of a completed invocation and translate the
/// completion into an exit code, the way a frontend maps `Outcome` /
/// `Trap` onto its own error surface.
fn report_snapshot(snapshot: &Snapshot) -> ExitCode {
    report_capture("stdout", &snapshot.stdout);
    report_capture("stderr", &snapshot.stderr);
    println!(
        "usage: wall_time={:.6}s memory_peak={} bytes",
        snapshot.usage.wall_time, snapshot.usage.memory_peak
    );

    match &snapshot.completion {
        Completion::Outcome(bytes) => match Outcome::decode(bytes) {
            Ok(Outcome::Value(value)) => {
                println!("=> {}", render(&value));
                ExitCode::SUCCESS
            }
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
    println!("{}", String::from_utf8_lossy(&capture.bytes));
}

/// A guest-side failure: the script raised (or was rejected) but the
/// invocation itself completed — the Ruby gem maps this to
/// `Kobako::SandboxError`.
fn report_panic(panic: &Panic) {
    eprintln!(
        "guest panic [{}] {}: {}",
        panic.origin, panic.class, panic.message
    );
    for line in &panic.backtrace {
        eprintln!("  {line}");
    }
}

/// An engine fault after the export started — wall-clock cap,
/// linear-memory cap, or any other wasm trap. Captures and usage above
/// survive it.
fn report_trap(trap: &Trap) {
    let kind = match trap {
        Trap::Timeout(_) => "timeout",
        Trap::MemoryLimit(_) => "memory limit",
        Trap::Other(_) => "trap",
    };
    eprintln!("guest {kind}: {trap}");
}

/// A failure that produced no invocation: the artifact is absent or
/// unusable (`ModuleNotBuilt` / `Dead`) or a host-side pre-call step
/// failed with the runtime still live (`Intact`).
fn report_setup_error(setup: &SetupError) {
    let (kind, msg) = match setup {
        SetupError::ModuleNotBuilt(msg) => ("guest artifact not built", msg),
        SetupError::Dead(msg) => ("runtime dead", msg),
        SetupError::Intact(msg) => ("setup failed", msg),
    };
    eprintln!("{kind}: {msg}");
}

/// The empty msgpack array a mandatory-presence stdin frame carries
/// when a host registers nothing.
fn empty_frame() -> Vec<u8> {
    let mut enc = Encoder::new();
    enc.write_value(&Value::Array(Vec::new()))
        .expect("an empty msgpack array always encodes");
    enc.into_bytes()
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
        Value::ErrEnv(bytes) => format!("<{} fault envelope bytes>", bytes.len()),
    }
}
