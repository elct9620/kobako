//! The per-invocation observable readout: print everything a
//! `Snapshot` carries and translate each completion / failure channel
//! into an exit code, the way a frontend maps `Outcome`, `Trap`, and
//! `SetupError` onto its own error surface.

use std::process::ExitCode;

use kobako_codec::codec::{Decode, Value};
use kobako_codec::outcome::{Outcome, Panic};
use kobako_runtime::error::{SetupError, Trap};
use kobako_runtime::snapshot::{Capture, Completion, Snapshot};

/// Print every observable of a completed invocation — captures, usage,
/// then the decoded completion.
pub fn report_snapshot(snapshot: &Snapshot) -> ExitCode {
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
pub fn report_trap(trap: &Trap) {
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
pub fn report_setup_error(setup: &SetupError) {
    let (kind, msg) = match setup {
        SetupError::ModuleNotBuilt(msg) => ("guest artifact not built", msg),
        SetupError::Dead(msg) => ("runtime dead", msg),
        SetupError::Intact(msg) => ("setup failed", msg),
    };
    eprintln!("{kind}: {msg}");
}

/// Render a decoded wire `Value` in Ruby `#inspect` style, so the
/// printed result reads like what the guest script returned.
pub fn render(value: &Value) -> String {
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
