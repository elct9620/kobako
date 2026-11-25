//! The most values one guest payload may make the host build, the codec's
//! own 262144 whatever the memory cap.

#![cfg(all(feature = "wasmtime", feature = "msgpack"))]

use crate::common::real_sandbox_with;
use kobako::msgpack::Value;
use kobako::{Error, Options};

// 1 + 512 arrays + 512 * 511 integers = 262145 values, one past the budget;
// popping one integer leaves exactly 262144.
const PAST_BUDGET: &str = "Array.new(512) { Array.new(511, 0) }";
const AT_BUDGET: &str = "a = Array.new(512) { Array.new(511, 0) }; a[0].pop; a";
const REFUSAL: &str = "malformed msgpack structure: payload holds more values than its budget";

fn answer_under(memory_limit: Option<usize>, script: &str) -> Option<Result<Value, Error>> {
    let mut options = Options::default();
    options.limits.memory_limit = memory_limit;
    let sandbox = real_sandbox_with(options)?;
    Some(sandbox.eval(script).and_then(|execution| execution.value()))
}

fn is_refused(answer: &Result<Value, Error>) -> bool {
    matches!(answer, Err(Error::Sandbox(failure))
        if failure.diagnostic.as_deref() == Some(REFUSAL))
}

// @behavior S-204
#[test]
fn an_answer_at_the_value_budget_reaches_the_host() {
    let Some(answer) = answer_under(Some(64 << 20), AT_BUDGET) else {
        return;
    };

    assert!(
        matches!(&answer, Ok(Value::Array(items)) if items.len() == 512),
        "an answer holding 262144 values through Execution::value must reach the Host App, \
         got {answer:?}"
    );
}

// @behavior S-204
#[test]
fn an_answer_past_the_value_budget_fails_whatever_the_memory_cap() {
    let answers: Option<Vec<_>> = [Some(64 << 20), None]
        .into_iter()
        .map(|memory_limit| answer_under(memory_limit, PAST_BUDGET))
        .collect();
    let Some(answers) = answers else {
        return;
    };

    assert!(
        answers.iter().all(is_refused),
        "an answer holding 262145 values through Execution::value under memory_limit 64 MiB or \
         none must fail as a Sandbox failure whose diagnostic names the budget, got {answers:?}"
    );
}

fn nan_keys(count: usize) -> String {
    format!("h = {{}}; {count}.times {{ h[0.0 / 0.0] = 0 }}; h")
}

// 500 NaN keys cost about 126 thousand values, 1000 about half a million.
// @behavior S-219
#[test]
fn a_hash_of_nan_keys_pays_for_comparing_them() {
    let (Some(within), Some(past)) = (
        answer_under(Some(64 << 20), &nan_keys(500)),
        answer_under(Some(64 << 20), &nan_keys(1000)),
    ) else {
        return;
    };

    assert!(
        matches!(&within, Ok(Value::Map(pairs)) if pairs.len() == 500),
        "a Hash of 500 NaN keys through Execution::value must reach the Host App, got {within:?}"
    );
    assert!(
        is_refused(&past),
        "a Hash of 1000 NaN keys through Execution::value must fail as a Sandbox failure whose \
         diagnostic names the budget, got {past:?}"
    );
}
