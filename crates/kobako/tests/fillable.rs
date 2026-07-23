//! Integration coverage for a fillable Service path (Ruby B-62): declared
//! with `bind_fillable` and left unfilled, it enters Frame 1 so the guest sees
//! the constant, but a dispatch to it fails closed as a Service failure — the
//! same fail-closed channel as an idiom with no backend. Driven through the
//! real guest binary; a missing binary is a hard failure under CI and a silent
//! skip locally, mirroring the Ruby E2E helper.

use std::path::Path;

use kobako::{Error, Options, Sandbox};

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

#[test]
fn an_unfilled_fillable_dispatch_fails_closed_as_a_service_error() {
    let Some(mut sandbox) = real_sandbox() else {
        return;
    };
    sandbox
        .bind_fillable("Store")
        .expect("declare a fillable path");

    let err = sandbox
        .eval("Store.get(1)")
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
    // observably distinct from the declared-but-unfilled fillable above.
    let err = sandbox
        .eval("Undeclared.get(1)")
        .expect_err("a reference to a never-declared constant must fail");

    assert!(
        matches!(err, Error::Sandbox(_)),
        "a never-declared constant is a guest-side Sandbox failure, distinct from a \
         fillable's Service failure (B-62), got {err:?}"
    );
}
