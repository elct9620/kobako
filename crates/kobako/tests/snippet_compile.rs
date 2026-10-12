//! A Sandbox has its preloaded source compiled once, at the seal, and
//! replays the bytecode afterwards. A runtime brought through the engine
//! seam decides what the compile answers, so these tests can give each
//! answer a real guest cannot be made to give on demand; the bundled guest
//! and a fixture whose compile never ends then show the first invocation
//! pays nothing for it.

// Only the cases behind the `wasmtime` feature load a Guest Binary; the
// others stand in any build, since the engine they drive is the one
// written here.
#[cfg(feature = "wasmtime")]
mod common;

use std::sync::{Arc, Mutex};

use kobako_runtime::dispatch::DispatchHandler;
use kobako_runtime::runtime::{Entry, Runtime};
use kobako_runtime::snapshot::{Capture, Completion, Snapshot, Usage};
use kobako_transport::envelope::{Outcome, Preamble, Snippet};

use kobako::{Profile, Sandbox};

/// Answers each compile with `answer` and records what every request and
/// every invocation was handed.
struct Compiling {
    answer: Option<Vec<Vec<u8>>>,
    compiles: Arc<Mutex<usize>>,
    replayed: Arc<Mutex<Vec<Vec<Snippet>>>>,
}

impl Runtime for Compiling {
    fn invoke(
        &self,
        _entry: Entry<'_>,
        preamble: &Preamble,
        _handler: Option<Arc<dyn DispatchHandler>>,
    ) -> Result<Snapshot, kobako_runtime::error::InvokeError> {
        self.replayed
            .lock()
            .unwrap()
            .push(preamble.snippets.entries.clone());
        Ok(Snapshot {
            completion: Completion::Outcome(Outcome::Ok(vec![0xc0])),
            stdout: Capture::default(),
            stderr: Capture::default(),
            usage: Usage::default(),
        })
    }

    fn profile(&self) -> Profile {
        Profile::Hermetic
    }

    fn compile(&self, _preamble: &Preamble) -> Option<Vec<Vec<u8>>> {
        *self.compiles.lock().unwrap() += 1;
        self.answer.clone()
    }
}

/// Two invocations of a Sandbox preloading one source snippet: how many
/// compiles it asked for, and the snippet table each invocation replayed.
fn invoked_twice(answer: Option<Vec<Vec<u8>>>) -> (usize, Vec<Vec<Snippet>>) {
    let compiles = Arc::new(Mutex::new(0));
    let replayed = Arc::new(Mutex::new(Vec::new()));
    let engine = Compiling {
        answer,
        compiles: compiles.clone(),
        replayed: replayed.clone(),
    };
    let mut sandbox = Sandbox::with_runtime(engine, Profile::Hermetic).unwrap();
    sandbox.preload("Alpha", "A = 1").unwrap();

    sandbox.eval("A").unwrap();
    sandbox.eval("A").unwrap();

    let compiles = *compiles.lock().unwrap();
    let replayed = replayed.lock().unwrap().clone();
    (compiles, replayed)
}

// @behavior S-186
#[test]
fn compiled_source_replays_as_bytecode_and_is_compiled_once() {
    let bytecode = Snippet::Bytecode {
        body: b"RITE\x00alpha".to_vec(),
    };

    assert_eq!(
        invoked_twice(Some(vec![b"RITE\x00alpha".to_vec()])),
        (1, vec![vec![bytecode.clone()], vec![bytecode]]),
        "a Sandbox through Sandbox::eval must compile its preloaded source once and replay \
         the bytecode the engine answered on every invocation"
    );
}

// @behavior S-187
#[test]
fn source_nothing_compiled_keeps_replaying_and_is_not_compiled_again() {
    let source = Snippet::Source {
        name: "Alpha".into(),
        body: "A = 1".into(),
    };

    assert_eq!(
        invoked_twice(None),
        (1, vec![vec![source.clone()], vec![source]]),
        "a Sandbox whose engine compiled nothing through Sandbox::eval must keep replaying \
         the source, with no second compile"
    );
}

// @behavior S-189
#[cfg(feature = "wasmtime")]
#[test]
fn compiling_counts_against_no_invocation() {
    let Some(mut sandbox) = common::real_sandbox() else {
        return;
    };
    let methods: Vec<String> = (1..=200).map(|i| format!("def self.m{i} = {i}")).collect();
    sandbox
        .preload("Big", &format!("module Big; {}; end", methods.join("; ")))
        .unwrap();

    let first = sandbox.eval("Big.m200").unwrap().usage().memory_peak;
    let second = sandbox.eval("Big.m200").unwrap().usage().memory_peak;

    assert_eq!(
        first, second,
        "the first Sandbox::eval of preloaded source must report the same memory peak as a \
         later one, since compiling that source is not part of any invocation"
    );
}

#[cfg(feature = "wasmtime")]
const DEADLINE: std::time::Duration = std::time::Duration::from_millis(300);

/// The first evaluation of a Sandbox over a guest whose compile spins until
/// the deadline: how long it took, and the Execution it returned.
#[cfg(feature = "wasmtime")]
fn first_eval_after_a_runaway_compile() -> (std::time::Duration, kobako::Execution) {
    const FIXTURE: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../test/fixtures/minimal_runaway_compile.wat"
    );
    let options = kobako::Options {
        timeout: Some(DEADLINE),
        memory_limit: None,
        ..kobako::Options::default()
    };
    let mut sandbox = Sandbox::new(FIXTURE, options).expect("the fixture must load");
    sandbox.preload("A", "A = 1").unwrap();

    let started = std::time::Instant::now();
    let execution = sandbox.eval("A").expect("the invocation must start");
    (started.elapsed(), execution)
}

// @behavior S-192
#[cfg(feature = "wasmtime")]
#[test]
fn a_compile_past_the_deadline_leaves_the_source_replaying() {
    let (elapsed, execution) = first_eval_after_a_runaway_compile();

    assert!(
        matches!(execution.payload(), Ok([0xc0])),
        "preloaded source whose compile runs past the deadline through Sandbox::eval must \
         still run the invocation on the source"
    );
    assert!(
        elapsed >= DEADLINE,
        "the compile through Sandbox::eval must run until the deadline stops it"
    );
}

// @behavior S-193
#[cfg(feature = "wasmtime")]
#[test]
fn compiling_spends_none_of_the_first_invocations_deadline() {
    let (_, execution) = first_eval_after_a_runaway_compile();

    assert!(
        execution.usage().wall_time < DEADLINE.as_secs_f64(),
        "a first Sandbox::eval after a compile that used up the deadline must complete and \
         report a wall time below that deadline"
    );
}
