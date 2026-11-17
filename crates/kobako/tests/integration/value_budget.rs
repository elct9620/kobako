//! The value budget a memory cap grants a guest payload, driven by a
//! fixture whose answer holds six values without growing its memory. The
//! real Guest Binary cannot reach the budget: encoding a value costs it
//! more memory than the budget grants.

#![cfg(all(feature = "wasmtime", feature = "msgpack"))]

use crate::common::real_sandbox_with;
use kobako::msgpack::Value;
use kobako::{Error, Options, Sandbox};

const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../test/fixtures/minimal_wide_outcome.wat"
);

fn answer_under(memory_limit: usize) -> Result<Value, Error> {
    let mut options = Options::default();
    options.limits.memory_limit = Some(memory_limit);
    let sandbox = Sandbox::new(FIXTURE, options).expect("the fixture must load");
    sandbox.eval("")?.value()
}

// 96 bytes of memory cap grant 96 / 16 = 6 values, exactly what the
// answer holds.
// @behavior S-204
#[test]
fn an_answer_at_the_value_budget_reaches_the_host() {
    assert_eq!(
        answer_under(96).expect("the answer must decode"),
        Value::Array(vec![Value::Int(0); 5]),
        "an answer holding as many values as memory_limit / 16 through Execution::value must \
         reach the Host App"
    );
}

// 80 bytes grant 5 values, one fewer than the answer holds.
// @behavior S-204
#[test]
fn an_answer_past_the_value_budget_is_a_sandbox_failure() {
    let refusal = answer_under(80);

    assert!(
        matches!(&refusal, Err(Error::Sandbox(failure))
            if failure.diagnostic.as_deref()
                == Some("malformed msgpack structure: payload holds more values than its budget")),
        "an answer holding more values than memory_limit / 16 through Execution::value must fail \
         as a Sandbox failure whose diagnostic names the budget, got {refusal:?}"
    );
}

fn nan_keys(count: usize) -> String {
    format!("h = {{}}; {count}.times {{ h[0.0 / 0.0] = 0 }}; h")
}

// 64 MiB grant 4194304 values: 1000 NaN keys cost about half a million,
// 4000 cost about eight million.
// @behavior S-219
#[test]
fn a_hash_of_nan_keys_pays_for_comparing_them() {
    let mut options = Options::default();
    options.limits.memory_limit = Some(64 << 20);
    let Some(sandbox) = real_sandbox_with(options) else {
        return;
    };

    let within = sandbox
        .eval(&nan_keys(1000))
        .expect("the guest ran")
        .value();
    assert!(
        matches!(&within, Ok(Value::Map(pairs)) if pairs.len() == 1000),
        "a Hash of 1000 NaN keys through Execution::value under memory_limit 64 MiB must reach the \
         Host App, got {within:?}"
    );
    let past = sandbox
        .eval(&nan_keys(4000))
        .and_then(|execution| execution.value());
    assert!(
        matches!(&past, Err(Error::Sandbox(failure))
            if failure.diagnostic.as_deref()
                == Some("malformed msgpack structure: payload holds more values than its budget")),
        "a Hash of 4000 NaN keys through Execution::value under memory_limit 64 MiB must fail as a \
         Sandbox failure whose diagnostic names the budget, got {past:?}"
    );
}
