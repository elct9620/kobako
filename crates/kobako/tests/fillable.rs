//! Integration coverage for a fillable Service path (Ruby B-62): declared
//! with `bind_fillable`, or as an Extension backend with `Provider::Fillable`,
//! and left unfilled, it enters Frame 1 so the guest sees the constant, but a
//! dispatch to it fails closed as a Service failure — the same fail-closed
//! channel as an idiom with no backend. A `ctx.bind` override fills it for one
//! invocation. Driven through the real guest binary; a missing binary is a hard
//! failure under CI and a silent skip locally, mirroring the Ruby E2E helper.
//!
//! The Services here are written against the bundled schema's overlay,
//! which is what makes each case readable, so the file stands with that
//! overlay. The byte-level path they share is walked in `byte_surface.rs`.
#![cfg(all(feature = "msgpack", feature = "wasmtime"))]

use std::path::Path;
use std::sync::Arc;

use kobako::msgpack::{Value, ValueReceiver};
use kobako::{Backend, Error, Extension, Fault, Handles, Options, Provider, Sandbox, Yielder};

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

/// A host store the guest reaches through the filled backend; `get` returns
/// its fixed value so a test can witness the fill.
struct Kv(&'static str);

impl ValueReceiver for Kv {
    fn call(
        &self,
        _method: &str,
        _args: &[Value],
        _kwargs: &[(String, Value)],
        _block: Option<&mut Yielder<'_>>,
        _handles: &Handles<'_>,
    ) -> Result<Value, Fault> {
        Ok(Value::Str(self.0.to_string()))
    }
}

/// An Extension whose backend is fillable — the guest idiom forwards every
/// call to the `Store` path, which stays unresolved until a `ctx.bind`
/// override supplies the object.
struct StoreExt;

impl Extension for StoreExt {
    fn name(&self) -> &str {
        "Store"
    }

    fn source(&self) -> &str {
        "class Store; extend Kobako::Proxy; end"
    }

    fn backend(&self) -> Option<Backend> {
        Some(Backend {
            path: "Store".to_string(),
            provider: Provider::Fillable,
        })
    }
}

#[test]
fn an_unfilled_fillable_dispatch_fails_closed_as_a_service_error() {
    let Some(mut sandbox) = real_sandbox() else {
        return;
    };
    sandbox
        .bind_fillable("Store")
        .expect("declare a fillable path");

    // The guest ran (so `eval` is `Ok`); the fillable failure is the run's
    // guest-level outcome, not a could-not-start.
    let err = sandbox
        .eval("Store.get(1)")
        .expect("the guest ran")
        .value()
        .expect_err("a dispatch to an unfilled fillable must fail closed (B-62)");

    assert!(
        matches!(err, Error::Service(_)),
        "an unfilled fillable's dispatch must surface as a Service failure (B-62), got {err:?}"
    );
}

#[test]
fn a_fillable_is_distinct_from_an_undeclared_constant() {
    let Some(mut sandbox) = real_sandbox() else {
        return;
    };
    sandbox
        .bind_fillable("Store")
        .expect("declare a fillable path");

    // A never-declared constant raises a guest NameError → Sandbox failure,
    // observably distinct from the declared-but-unfilled fillable above. The
    // guest ran, so the failure rides the returned Execution's outcome.
    let err = sandbox
        .eval("Undeclared.get(1)")
        .expect("the guest ran")
        .value()
        .expect_err("a reference to a never-declared constant must fail");

    assert!(
        matches!(err, Error::Sandbox(_)),
        "a never-declared constant is a guest-side Sandbox failure, distinct from a \
         fillable's Service failure (B-62), got {err:?}"
    );
}

#[test]
fn an_extension_fillable_backend_left_unfilled_fails_closed() {
    let Some(mut sandbox) = real_sandbox() else {
        return;
    };
    sandbox
        .install(Arc::new(StoreExt))
        .expect("install the fillable-backed Extension");

    let err = sandbox
        .eval("Store.get(1)")
        .expect("the guest ran")
        .value()
        .expect_err("a dispatch to an unfilled Extension fillable backend must fail closed (B-56)");

    assert!(
        matches!(err, Error::Service(_)),
        "an unfilled Provider::Fillable backend's dispatch must surface as a Service failure \
         (B-56), got {err:?}"
    );
}

#[test]
fn a_ctx_bind_override_fills_an_extension_fillable_backend() {
    let Some(mut sandbox) = real_sandbox() else {
        return;
    };
    sandbox
        .install(Arc::new(StoreExt))
        .expect("install the fillable-backed Extension");

    let value = sandbox
        .eval_with("Store.get(1)", |ctx| {
            ctx.bind("Store", Kv("filled").into_receiver())
        })
        .expect("the guest ran")
        .value()
        .expect("a filled Extension backend dispatches to the override object (B-56 / B-63)");

    assert_eq!(
        value,
        Value::Str("filled".into()),
        "a ctx.bind override must fill a Provider::Fillable backend so the guest reaches the \
         supplied object (B-56 / B-63)"
    );
}
