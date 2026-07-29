//! `Arc<Sandbox>` drives concurrent `&self` invocations, shared-nothing: each
//! `eval` builds its own per-invocation state (Handle table, observables) and
//! returns its own `Execution`, so results never cross between threads. The
//! Ruby frontend reaches concurrency through `Kobako::Pool` instead — a
//! deliberate per-language divergence; the SDK leans on the borrow checker to
//! prove the shared config is only read. Driven through the real guest binary;
//! a missing binary is a hard failure under CI and a silent skip locally.

// Driven through the bundled engine: these cases load a real Guest Binary,
// so they stand only in a build that carries one.
#![cfg(feature = "wasmtime")]

use std::path::Path;
use std::sync::Arc;
use std::thread;

// The schema read here is the guest's, not this crate's: the bundled
// binary answers in MessagePack however the SDK was built, so the codec
// comes from the crate that owns that schema rather than from a feature.
use kobako_codec::msgpack::codec::{Decoder, Value};

use kobako::{Options, Sandbox};

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

/// Read one outcome the way a host owning its own schema does — off the
/// bytes, with the decoder for the schema the guest speaks.
fn decode(bytes: &[u8]) -> Value {
    Decoder::new(bytes)
        .read_only_value()
        .expect("the guest answers in the schema it was built with")
}

#[test]
fn arc_sandbox_drives_concurrent_evals_shared_nothing() {
    let Some(sandbox) = real_sandbox() else {
        return;
    };
    // Sharing the sealed config across threads is exactly what `&self` eval
    // and `Arc<Sandbox>` are for; the first concurrent evals also race on the
    // seal, so this exercises the locked seal path.
    let sandbox = Arc::new(sandbox);

    let threads: Vec<_> = (0..8)
        .map(|n| {
            let sandbox = Arc::clone(&sandbox);
            thread::spawn(move || {
                let execution = sandbox.eval(&format!("{n} * {n}")).expect("the guest ran");
                let bytes = execution
                    .payload()
                    .expect("a concurrent eval returns its own value");
                (n, decode(bytes))
            })
        })
        .collect();

    for thread in threads {
        let (n, value) = thread.join().expect("a concurrent eval thread panicked");
        assert_eq!(
            value,
            Value::Int(i64::from(n * n)),
            "each concurrent eval on a shared Arc<Sandbox> must return its own result, \
             proving per-invocation state never crosses threads"
        );
    }
}
