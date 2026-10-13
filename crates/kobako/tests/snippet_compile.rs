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

/// Answers each compile with `answer`, or panics when `panics` is set, and
/// records what every request and every invocation was handed.
#[derive(Default)]
struct Compiling {
    answer: Option<Vec<Vec<u8>>>,
    panics: bool,
    compiles: Arc<Mutex<usize>>,
    replayed: Replayed,
}

/// The snippet table each invocation was handed, in invocation order.
type Replayed = Arc<Mutex<Vec<Vec<Snippet>>>>;

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
        assert!(!self.panics, "the engine fails this compile outright");
        self.answer.clone()
    }
}

/// A Sandbox over `engine` preloading one source snippet, with the
/// counters the engine shares.
fn preloading(engine: Compiling) -> (Sandbox, Arc<Mutex<usize>>, Replayed) {
    let (compiles, replayed) = (engine.compiles.clone(), engine.replayed.clone());
    let mut sandbox = Sandbox::with_runtime(engine, Profile::Hermetic).unwrap();
    sandbox.preload("Alpha", "A = 1").unwrap();
    (sandbox, compiles, replayed)
}

/// Two invocations of a Sandbox preloading one source snippet: how many
/// compiles it asked for, and the snippet table each invocation replayed.
fn invoked_twice(answer: Option<Vec<Vec<u8>>>) -> (usize, Vec<Vec<Snippet>>) {
    let (sandbox, compiles, replayed) = preloading(Compiling {
        answer,
        ..Compiling::default()
    });

    sandbox.eval("A").unwrap();
    sandbox.eval("A").unwrap();

    let compiles = *compiles.lock().unwrap();
    let replayed = replayed.lock().unwrap().clone();
    (compiles, replayed)
}

fn alpha_source() -> Snippet {
    Snippet::Source {
        name: "Alpha".into(),
        body: "A = 1".into(),
    }
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
    let source = alpha_source();

    assert_eq!(
        invoked_twice(None),
        (1, vec![vec![source.clone()], vec![source]]),
        "a Sandbox whose engine compiled nothing through Sandbox::eval must keep replaying \
         the source, with no second compile"
    );
}

// The panic stands in for any engine fault the compile cannot answer
// around; the Sandbox must outlive it rather than refuse every later call.
// @behavior S-195
#[test]
fn a_compile_that_panics_leaves_the_source_replaying() {
    let (sandbox, _, replayed) = preloading(Compiling {
        panics: true,
        ..Compiling::default()
    });

    let first = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| sandbox.eval("A")));
    let second = sandbox.eval("A");

    assert_eq!(
        (
            first.is_err(),
            second.is_ok(),
            replayed.lock().unwrap().clone()
        ),
        (true, true, vec![vec![alpha_source()]]),
        "a Sandbox whose engine panics compiling its preloaded source through Sandbox::eval must \
         fail that invocation with the panic and run the next one on the source"
    );
}

/// A guest idiom declaring a dependency nothing installs.
struct NeedsErrno;

impl kobako::Extension for NeedsErrno {
    fn name(&self) -> &str {
        "File"
    }

    fn source(&self) -> &str {
        "class File; end"
    }

    fn depends_on(&self) -> &[&str] {
        &["Errno"]
    }
}

// @behavior S-196
#[test]
fn an_unmet_dependency_is_refused_before_any_compile() {
    let (mut sandbox, compiles, _) = preloading(Compiling::default());
    sandbox.install(Arc::new(NeedsErrno)).unwrap();

    let refused = sandbox.eval("A").is_err();

    assert_eq!(
        (refused, *compiles.lock().unwrap()),
        (true, 0),
        "a Sandbox with preloaded source and an unmet Extension dependency through Sandbox::eval \
         must be refused without asking the engine to compile"
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

/// Two evaluations of a module whose bytecode is about three times its 4 KiB
/// of source, preloaded beside `bound` with a flag leaving `free` bytes of
/// the Preamble's 16 MiB; `None` when a local checkout has no Guest Binary.
#[cfg(feature = "wasmtime")]
fn big_module_answers(free: usize, bound: Option<&str>) -> Option<Vec<String>> {
    let options = kobako::Options {
        memory_limit: None,
        ..kobako::Options::default()
    };
    let mut sandbox = common::real_sandbox_with(options)?;
    let methods: Vec<String> = (1..=200).map(|i| format!("def self.m{i} = {i}")).collect();
    sandbox
        .flag_advisory("pad", &"x".repeat((16 << 20) - free))
        .unwrap();
    if let Some(path) = bound {
        sandbox.bind_fillable(path).unwrap();
    }
    sandbox
        .preload("Big", &format!("module Big; {}; end", methods.join("; ")))
        .unwrap();
    Some(
        (0..2)
            .map(|_| match sandbox.eval("Big.m200") {
                Ok(execution) => format!("{:?}", execution.payload()),
                Err(err) => format!("{err:?}"),
            })
            .collect(),
    )
}

// 8 KiB fits the source with room to spare and its bytecode with none.
// @behavior S-199
#[cfg(feature = "wasmtime")]
#[test]
fn bytecode_the_preamble_could_not_carry_leaves_the_source_replaying() {
    let Some(answers) = big_module_answers(8 << 10, None) else {
        return;
    };

    assert!(
        answers.iter().all(|answer| answer == "Ok([204, 200])"),
        "preloaded source whose bytecode would not fit the Preamble through Sandbox::eval must \
         keep replaying as the source that does, got {answers:?}"
    );
}

// The same module beside a 6 KiB bound path, with a flag leaving 14 KiB:
// its bytecode fits alone and not beside the bindings, so a compile that
// left the bound paths out of its Preamble would keep an answer that traps.
// @behavior S-199
#[cfg(feature = "wasmtime")]
#[test]
fn bytecode_that_fits_only_without_the_bindings_leaves_the_source_replaying() {
    let path = format!("A{}", "b".repeat(6 << 10));
    let Some(answers) = big_module_answers(14 << 10, Some(&path)) else {
        return;
    };

    assert!(
        answers.iter().all(|answer| answer == "Ok([204, 200])"),
        "preloaded source whose bytecode fits the Preamble only without its bound paths through \
         Sandbox::eval must keep replaying as the source, got {answers:?}"
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
