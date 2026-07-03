//! Dispatch-capable kobako host: the `minimal` assembly plus the
//! guest→host half.
//!
//! Frame 1 registers the `MyService::KV` constant path, and a
//! `DispatchHandler` answers each `__kobako_dispatch` the guest makes
//! against it — decode the `Request` envelope, route to an in-process
//! store, encode a `Response`. The handler honours the dispatch
//! contract the Ruby gem's `Transport::Dispatcher` pins: it never
//! fails, folding every failure into a `Response::Err` fault the guest
//! re-raises as its own error, so a misuse of the Service surfaces as
//! a rescuable guest exception rather than a wasm trap.

use std::collections::HashMap;
use std::env;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use kobako_codec::codec::{Decode, Encode, Encoder, Value};
use kobako_codec::transport::{Request, Response, Target};
use kobako_runtime::dispatch::DispatchHandler;
use kobako_runtime::error::Error;
use kobako_runtime::profile::Profile;
use kobako_runtime::runtime::{Entry, Frames, Runtime};
use kobako_runtime::yielder::Yielder;
use kobako_wasmtime::{Config, Driver};

use kobako_rust_host::empty_frame;
use kobako_rust_host::report::{render, report_setup_error, report_snapshot, report_trap};

/// Demo source when none is given on the command line: a round-trip
/// through the store, a rescued Service fault, and a miss returning
/// `nil` — the three answers a dispatch can come back with.
const DEFAULT_SOURCE: &str = r#"
MyService::KV.set("greeting", "hello via dispatch")
puts MyService::KV.get("greeting")

begin
  MyService::KV.remove("greeting")
rescue => e
  puts "rescued: #{e.class}: #{e.message}"
end

MyService::KV.get("missing")
"#;

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let Some(wasm_path) = args.next().map(PathBuf::from) else {
        eprintln!("usage: services <path/to/kobako.wasm> [mruby-source]");
        return ExitCode::FAILURE;
    };
    let source = args
        .next()
        .unwrap_or_else(|| DEFAULT_SOURCE.trim().to_string());

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

    // Frame 1 carries the registration preamble
    // `[["Namespace", ["Member", ...]], ...]`; the guest installs a
    // proxy constant for each entry, so guest code reaches the store
    // as plain `MyService::KV` calls.
    let preamble = kv_preamble();
    let snippets = empty_frame();
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
        Err(Error::Setup(setup)) => {
            report_setup_error(&setup);
            ExitCode::FAILURE
        }
        Err(Error::Trap(trap)) => {
            report_trap(&trap);
            ExitCode::FAILURE
        }
    };

    // The store outlives the invocation on the host side — the state
    // the guest mutated through dispatch is ordinary host state.
    for (key, value) in handler.entries() {
        println!("host store: {key:?} => {}", render(&value));
    }
    exit
}

/// The Frame 1 preamble registering `MyService::KV`.
fn kv_preamble() -> Vec<u8> {
    let group = Value::Array(vec![
        Value::Str("MyService".into()),
        Value::Array(vec![Value::Str("KV".into())]),
    ]);
    let mut enc = Encoder::new();
    enc.write_value(&Value::Array(vec![group]))
        .expect("a str/array preamble always encodes");
    enc.into_bytes()
}

/// An in-process key-value store exposed to the guest as
/// `MyService::KV` — the host side of every dispatch the demo source
/// makes.
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

    /// Route one decoded `Request` to the store, mirroring the fault
    /// taxonomy the Ruby dispatcher uses: `undefined` for an unknown
    /// target or method, `argument` for a call shape the method does
    /// not take.
    fn handle(&self, request: &Request) -> Response {
        let Target::Path(path) = &request.target else {
            return fault("undefined", "this host allocates no Capability Handles");
        };
        if path != "MyService::KV" {
            return fault("undefined", &format!("unknown constant {path}"));
        }
        if !request.kwargs.is_empty() {
            return fault("argument", "KV methods accept no keyword arguments");
        }
        match (request.method.as_str(), request.args.as_slice()) {
            ("get", [Value::Str(key)]) => {
                let value = self.lock_store().get(key).cloned().unwrap_or(Value::Nil);
                Response::Ok(value)
            }
            ("set", [Value::Str(key), value]) => {
                self.lock_store().insert(key.clone(), value.clone());
                Response::Ok(value.clone())
            }
            ("get" | "set", _) => fault(
                "argument",
                "get takes one String key; set takes a String key and a value",
            ),
            (method, _) => fault(
                "undefined",
                &format!("method :{method} is not a Service method"),
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
    /// reifies every failure as a `Response::Err` instead, so the guest
    /// always receives an envelope.
    fn dispatch(&self, request: &[u8], _yielder: &mut dyn Yielder) -> Option<Vec<u8>> {
        let response = match Request::decode(request) {
            Ok(request) => self.handle(&request),
            Err(err) => fault(
                "runtime",
                &format!("Sandbox received a malformed request: {err}"),
            ),
        };
        let bytes = response.encode().unwrap_or_else(|err| {
            // A value the wire cannot carry back (e.g. nested past the
            // depth cap) folds like every other failure; the flat fault
            // map itself always encodes.
            fault("runtime", &format!("response not encodable: {err}"))
                .encode()
                .expect("a flat fault map always encodes")
        });
        Some(bytes)
    }
}

/// A `Response::Err` carrying the ext 0x02 fault payload — a msgpack
/// map of `type` (which proxy-side error the guest raises) and
/// `message`.
fn fault(kind: &str, message: &str) -> Response {
    let mut enc = Encoder::new();
    enc.write_value(&Value::Map(vec![
        (Value::Str("type".into()), Value::Str(kind.into())),
        (Value::Str("message".into()), Value::Str(message.into())),
    ]))
    .expect("a str/str fault map always encodes");
    Response::Err(enc.into_bytes())
}
