//! The value budget a memory cap grants a guest payload, driven by a
//! fixture whose answer holds six values without growing its memory. The
//! real Guest Binary cannot reach the budget: encoding a value costs it
//! more memory than the budget grants.

#![cfg(all(feature = "wasmtime", feature = "msgpack"))]

use kobako::msgpack::Value;
use kobako::{Error, Options, Sandbox};

const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../test/fixtures/minimal_wide_outcome.wat"
);

fn answer_under(memory_limit: usize) -> Result<Value, Error> {
    let options = Options {
        memory_limit: Some(memory_limit),
        ..Options::default()
    };
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
