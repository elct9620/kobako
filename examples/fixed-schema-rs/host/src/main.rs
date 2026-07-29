//! A kobako host whose Service speaks its own schema.
//!
//! Run it against the Guest Binary the sibling `guest/` tree builds:
//!
//! ```console
//! $ cargo run --release -- ../guest.wasm            # the demo
//! $ cargo run --release -- ../guest.wasm --run      # a preloaded entrypoint
//! $ cargo run --release -- ../guest.wasm --check    # the claims
//! $ cargo run --release -- ../guest.wasm '<script>' # anything else
//! ```

mod check;
mod entry;
mod schema;
mod session;
mod store;

use std::env;
use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use kobako::{Options, Sandbox};

use crate::store::{ProtoKv, Store};

/// The path the guest gem's methods dispatch to, declared here at setup
/// and filled per invocation below.
const KV_PATH: &str = "MyService::KV";

/// What the demo runs. Every call reaches the host under the fixed
/// schema, and the script ends on a String because that is the one shape
/// this guest's codec carries out of an invocation.
const DEMO: &str = r#"
fresh   = MyService::KV.put("greeting", "hello")
again   = MyService::KV.put("greeting", "hi")
value   = MyService::KV.get("greeting")
missing = MyService::KV.get("absent")

# A Ruby String is a byte string, and the schema carries `bytes`. The
# same bytes stand in both positions, so the lookup only hits if the key
# survived and only matches if the value did — a schema built on
# `string` fields could promise neither.
binary = "bin\xffkey"
MyService::KV.put(binary, binary)
kept = MyService::KV.get(binary)

# A Handle, reached both ways it can be. `session.put` targets the id the
# host issued, and `KV.count(session)` passes that same id back as an
# argument — one table on the host answers both.
session = MyService::KV.open("tenant/")
session.put("a", "1")
session.put("b", "2")
scoped = session.get("a")
mine   = MyService::KV.count(session)

"put=#{fresh}/#{again} get=#{value.inspect} miss=#{missing.inspect} " \
"binary_intact=#{kept == binary} session_get=#{scoped.inspect} session_keys=#{mine}"
"#;

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let Some(wasm_path) = args.next() else {
        eprintln!("usage: kv-host <path/to/guest.wasm> [--check | <script>]");
        return ExitCode::FAILURE;
    };

    match run(Path::new(&wasm_path), args.next().as_deref()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn run(wasm_path: &Path, mode: Option<&str>) -> Result<(), String> {
    let sandbox = open(wasm_path)?;
    match mode {
        Some("--check") => check::run(&sandbox),
        Some("--run") => entry::serve(&sandbox, b"hello from the host"),
        source => demo(&sandbox, source.unwrap_or(DEMO)),
    }
}

/// Load the guest, declare the Service the gem's methods reach, and
/// preload the entrypoint `--run` calls.
fn open(wasm_path: &Path) -> Result<Sandbox, String> {
    let mut sandbox = Sandbox::new(
        wasm_path,
        Options {
            timeout: Some(Duration::from_secs(30)),
            ..Options::default()
        },
    )
    .map_err(|err| format!("cannot load the guest: {err}"))?;

    // Declared without an object: the schema is fixed at build time, and
    // the receiver behind the path is not. A multi-tenant host fills it
    // with a different object every invocation, and the guest's methods
    // do not change.
    sandbox
        .bind_fillable(KV_PATH)
        .map_err(|err| format!("cannot declare {KV_PATH}: {err}"))?;
    sandbox
        .preload(entry::NAME, entry::SOURCE)
        .map_err(|err| format!("cannot preload {}: {err}", entry::NAME))?;
    Ok(sandbox)
}

/// Run one script against a fresh store and print what came back.
fn demo(sandbox: &Sandbox, source: &str) -> Result<(), String> {
    let store = Arc::new(Store::default());
    let execution = sandbox
        .eval_with(source, |ctx| {
            ctx.bind(KV_PATH, Arc::new(ProtoKv(store.clone())))
        })
        .map_err(|err| format!("the invocation did not start: {err}"))?;

    // The outcome is bytes, not a decoded value: this host's schema is
    // its own, so it reads its own result here. The demo script ends on
    // a String, and the codec carries one as its own bytes.
    match execution.payload() {
        Ok(bytes) => println!("guest returned: {}", String::from_utf8_lossy(bytes)),
        Err(failure) => return Err(format!("the guest failed: {failure}")),
    }
    println!("host store now holds {} key(s)", store.count());
    Ok(())
}
