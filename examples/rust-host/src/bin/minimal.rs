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

use kobako_runtime::error::Error;
use kobako_runtime::profile::Profile;
use kobako_runtime::runtime::{Entry, Frames, Runtime};
use kobako_wasmtime::{Config, Driver};

use kobako_rust_host::empty_frame;
use kobako_rust_host::report::{report_setup_error, report_snapshot, report_trap};

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
