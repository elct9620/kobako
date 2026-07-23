//! Integration coverage for the per-invocation override closure (Ruby B-63):
//! `eval_with` fills a fillable or shadows a declared binding for one
//! invocation, and refuses an undeclared override before the guest runs.
//! Driven through the real guest binary; a missing binary is a hard failure
//! under CI and a silent skip locally, mirroring the Ruby E2E helper.

use std::path::Path;
use std::sync::Arc;

use kobako::{Error, Fault, Handles, Options, Receiver, Sandbox, Value, Yielder};

const WASM: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/kobako.wasm");

/// A host store the guest reaches as the bound constant; `get` returns its
/// fixed value so a test can witness which object backed the path.
struct Kv(&'static str);

impl Receiver for Kv {
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

#[test]
fn eval_with_fills_a_fillable_for_the_invocation() {
    let Some(mut sandbox) = real_sandbox() else {
        return;
    };
    sandbox
        .bind_fillable("Store")
        .expect("declare a fillable path");

    let value = sandbox
        .eval_with("Store.get(1)", |ctx| {
            ctx.bind("Store", Arc::new(Kv("filled")))
        })
        .expect("a filled fillable must dispatch to the override object (B-63)");

    assert_eq!(
        value,
        Value::Str("filled".into()),
        "eval_with must fill the fillable so the guest reaches the override object (B-63)"
    );
}

#[test]
fn eval_with_shadows_a_static_binding_for_one_invocation_only() {
    let Some(mut sandbox) = real_sandbox() else {
        return;
    };
    sandbox
        .bind("Store", Arc::new(Kv("base")))
        .expect("bind a static Service");

    let overridden = sandbox
        .eval_with("Store.get(1)", |ctx| {
            ctx.bind("Store", Arc::new(Kv("override")))
        })
        .expect("an override shadows the static binding");
    let plain = sandbox
        .eval("Store.get(1)")
        .expect("the base binding resolves without an override");

    assert_eq!(
        overridden,
        Value::Str("override".into()),
        "an override must shadow the static binding for this invocation (B-63)"
    );
    assert_eq!(
        plain,
        Value::Str("base".into()),
        "the override lasts only its own invocation; the next eval sees the base binding (B-63)"
    );
}

#[test]
fn eval_with_rejects_an_undeclared_override_before_the_guest_runs() {
    let Some(mut sandbox) = real_sandbox() else {
        return;
    };
    sandbox
        .bind_fillable("Store")
        .expect("declare a fillable path");

    let err = sandbox
        .eval_with("1", |ctx| ctx.bind("Undeclared", Arc::new(Kv("x"))))
        .expect_err("overriding an undeclared path must fail before the guest runs (B-63)");

    assert!(
        matches!(err, Error::Argument(_)),
        "an undeclared override must surface as Error::Argument, keeping the key set fixed (B-63), got {err:?}"
    );
}
