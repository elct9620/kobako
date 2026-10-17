//! Host-side magnus shell over the extracted wasmtime driver.
//!
//! The Ruby-visible classes are
//!
//!   Kobako::Runtime           — wraps a `kobako_wasmtime::Driver`
//!   Kobako::Runtime::Snapshot — one invocation's completion + captures + usage
//!
//! `Kobako::Runtime` is constructed via `Kobako::Runtime.from_path(path,
//! **options)`, the options keyed by the `Kobako::SandboxOptions` members. Every
//! invocation (`#eval` / `#run`) takes that run's dispatch handler as a call
//! argument, instantiates a fresh instance, and returns a `Snapshot` — the
//! whole per-invocation result — so the Runtime holds no per-invocation
//! state and one Runtime is safe to drive concurrently. The run mechanics —
//! engine/module caches, caps, trap classification — live in the
//! `kobako-wasmtime` crate behind the `kobako_runtime` contract; no wasm
//! engine type reaches this crate or the Host App.
//!
//! Each module holds one responsibility and opens with its own doc.

mod bridge;
mod errors;
mod gvl;

use magnus::{
    function, method, prelude::*, scan_args::get_kwargs, typed_data::DataTypeFunctions,
    value::Opaque, Error as MagnusError, Exception, RArray, RHash, RModule, RString, Ruby, Symbol,
    TypedData, Value,
};

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use kobako_runtime::dispatch::DispatchHandler;
use kobako_runtime::profile::Profile;
use kobako_runtime::runtime::{Entry, Runtime as ContractRuntime};
use kobako_runtime::snapshot::{Completion, Snapshot as RuntimeSnapshot};
use kobako_transport::envelope::{Bindings, Flag, Outcome, Preamble, Snippet, Snippets};
use kobako_wasmtime::{Config, Driver};

/// A Panic's fields as they cross to Ruby: origin, class, message,
/// backtrace, and the names the invocation could have used in place of the
/// one it named. Named so the Ruby side reads them positionally against one
/// shape, matching the `Kobako::Outcome::panic_fields` alias.
type PanicFields = (String, String, String, Vec<String>, Vec<String>);

/// The one place the boundary borrows an RString's bytes: the borrow does
/// not outlive this call, so no Ruby allocation can move the RString
/// between the borrow and the copy.
fn rstring_to_vec(s: RString) -> Vec<u8> {
    // SAFETY: see item doc.
    unsafe { s.as_slice() }.to_vec()
}

/// `kind` arrives as a Symbol so the registry never names the wire's
/// discriminant; an off-ladder kind raises rather than defaulting.
fn snippets_from(ruby: &Ruby, entries: RArray) -> Result<Snippets, MagnusError> {
    let mut snippets = Snippets {
        entries: Vec::with_capacity(entries.len()),
    };
    for index in 0..entries.len() as isize {
        let entry: RArray = entries.entry(index)?;
        let kind: Symbol = entry.entry(0)?;
        snippets.entries.push(match kind.name()?.as_ref() {
            "source" => Snippet::Source {
                name: entry.entry(1)?,
                body: entry.entry(2)?,
            },
            "bytecode" => Snippet::Bytecode {
                body: rstring_to_vec(entry.entry(2)?),
            },
            other => {
                return Err(MagnusError::new(
                    ruby.exception_arg_error(),
                    format!("snippet kind must be :source or :bytecode, got :{other}"),
                ))
            }
        });
    }
    Ok(snippets)
}

/// One owner for the Preamble every guest entry reads, so an invocation
/// and a compile cannot drift in what they send.
fn preamble_from(
    ruby: &Ruby,
    paths: Vec<String>,
    snippets: RArray,
    flags: RArray,
) -> Result<Preamble, MagnusError> {
    let mut preamble = Preamble::default();
    preamble.bindings = Bindings { paths };
    preamble.snippets = snippets_from(ruby, snippets)?;
    preamble.flags = flags
        .to_vec::<(String, String, bool)>()?
        .into_iter()
        .map(|(name, value, critical)| Flag {
            name,
            value,
            critical,
        })
        .collect();
    Ok(preamble)
}

// ---------------------------------------------------------------------------
// Ruby init
// ---------------------------------------------------------------------------

pub fn init(ruby: &Ruby, kobako: RModule) -> Result<(), MagnusError> {
    // Error hierarchy lives in `lib/kobako/errors.rb`; the ext raises
    // directly into those classes through the constructors and mappers
    // in `runtime/errors.rs` — no intermediate hierarchy is registered.

    let runtime = kobako.define_class("Runtime", ruby.class_object())?;
    runtime.define_singleton_method("from_path", function!(Runtime::from_path, 2))?;
    runtime.define_method("eval", method!(Runtime::eval, 5))?;
    runtime.define_method("run", method!(Runtime::run, 6))?;
    runtime.define_method("profile", method!(Runtime::profile, 0))?;
    runtime.define_method("compile", method!(Runtime::compile, 3))?;
    // The guest re-enters for a block yield through a frame-scoped
    // `Kobako::Runtime::GuestYielder` the dispatcher hands the Proc, not a
    // method on Runtime.
    bridge::register(runtime)?;

    // Snapshot — the per-invocation result object each entry point returns.
    let snapshot = runtime.define_class("Snapshot", ruby.class_object())?;
    snapshot.define_method("outcome", method!(Snapshot::outcome, 0))?;
    snapshot.define_method("trap_error", method!(Snapshot::trap_error, 0))?;
    snapshot.define_method("wall_time", method!(Snapshot::wall_time, 0))?;
    snapshot.define_method("memory_peak", method!(Snapshot::memory_peak, 0))?;
    snapshot.define_method("stdout", method!(Snapshot::stdout, 0))?;
    snapshot.define_method("stdout_truncated?", method!(Snapshot::stdout_truncated, 0))?;
    snapshot.define_method("stderr", method!(Snapshot::stderr, 0))?;
    snapshot.define_method("stderr_truncated?", method!(Snapshot::stderr_truncated, 0))?;

    Ok(())
}

#[derive(TypedData)]
#[magnus(class = "Kobako::Runtime", free_immediately, size)]
struct Runtime {
    // The magnus-free wasmtime driver that runs every invocation; the
    // shell only shuttles Ruby values across its boundary. The Runtime
    // holds no per-invocation state — each `#eval` / `#run` takes its
    // dispatch handler as an argument and returns its whole result as a
    // `Snapshot` — so `Driver`'s own `Send + Sync` carries the type with no
    // interior mutability to guard.
    driver: Driver,
    // Whether each invocation releases Ruby's GVL for its guest span
    // (`gvl: :release`) or holds it throughout (`gvl: :hold`). Fixed at
    // construction; a `bool` carries no interior mutability, so the type
    // stays `Send + Sync`.
    release_gvl: bool,
}

impl DataTypeFunctions for Runtime {}

impl Runtime {
    /// The only Ruby-facing constructor, so Engine and Module are never
    /// visible to Ruby. Every option is a required keyword: the defaults
    /// belong to `Kobako::SandboxOptions`, which names the keys.
    fn from_path(path: String, options: RHash) -> Result<Self, MagnusError> {
        let ruby = Ruby::get().expect("Ruby thread");
        type Options = (
            Option<f64>,
            Option<usize>,
            Option<usize>,
            Option<usize>,
            Symbol,
            Symbol,
        );
        let options = get_kwargs::<_, Options, (), ()>(
            options,
            &[
                "timeout",
                "memory_limit",
                "stdout_limit",
                "stderr_limit",
                "profile",
                "gvl",
            ],
            &[],
        )?;
        let (timeout_seconds, memory_limit, stdout_limit, stderr_limit, profile, gvl) =
            options.required;
        let timeout = match timeout_seconds {
            None => None,
            Some(secs) if secs.is_finite() && secs > 0.0 => Some(Duration::from_secs_f64(secs)),
            Some(secs) => {
                // An invalid cap argument is a Host App
                // programming error and raises `ArgumentError`, outside the
                // construction-failure `SetupError` branch. `SandboxOptions`
                // is the primary guard (it never lets a bad timeout reach
                // here); this is defence-in-depth for direct `from_path` calls.
                return Err(MagnusError::new(
                    ruby.exception_arg_error(),
                    format!("timeout must be > 0 and finite, got {secs} seconds"),
                ));
            }
        };
        // Fail closed on an off-ladder rung: an unrecognized posture must
        // never fall back to a weaker posture. Same defence in depth as the
        // timeout guard above — `SandboxOptions` is the primary validator.
        let profile = match profile.name()?.as_ref() {
            "hermetic" => Profile::Hermetic,
            "permissive" => Profile::Permissive,
            other => {
                return Err(MagnusError::new(
                    ruby.exception_arg_error(),
                    format!("profile must be :permissive or :hermetic, got :{other}"),
                ));
            }
        };
        // Same fail-closed posture as `profile`: an unrecognized mode raises
        // rather than defaulting. `SandboxOptions` is the primary validator;
        // this guards direct `from_path` calls.
        let release_gvl = match gvl.name()?.as_ref() {
            "hold" => false,
            "release" => true,
            other => {
                return Err(MagnusError::new(
                    ruby.exception_arg_error(),
                    format!("gvl must be :hold or :release, got :{other}"),
                ));
            }
        };

        let driver = Driver::new(
            Path::new(&path),
            Config {
                timeout,
                memory_limit,
                stdout_limit,
                stderr_limit,
                profile,
            },
        )
        .map_err(|e| errors::setup_to_magnus(&ruby, e))?;
        Ok(Self {
            driver,
            release_gvl,
        })
    }

    // -----------------------------------------------------------------
    // Run-path methods. Each takes the run's dispatch handler as its first
    // argument and returns a `Snapshot` for any completed invocation —
    // success or trap alike. Only a could-not-start fault (a missing export
    // or a fault before the export call) raises directly, as the class
    // `errors::to_magnus` gives its cause, since it yields no `Snapshot`.
    // -----------------------------------------------------------------

    fn eval(
        &self,
        dispatch: Value,
        paths: RArray,
        snippets: RArray,
        flags: RArray,
        source: RString,
    ) -> Result<Snapshot, MagnusError> {
        let source = rstring_to_vec(source);
        self.invoke(
            dispatch,
            paths,
            snippets,
            flags,
            Entry::Eval { source: &source },
        )
    }

    fn run(
        &self,
        dispatch: Value,
        paths: RArray,
        snippets: RArray,
        flags: RArray,
        entrypoint: String,
        payload: RString,
    ) -> Result<Snapshot, MagnusError> {
        self.invoke(
            dispatch,
            paths,
            snippets,
            flags,
            Entry::Run {
                entrypoint: &entrypoint,
                payload: rstring_to_vec(payload),
            },
        )
    }

    /// One owner for the wiring both verbs share, so a Preamble or handler
    /// change cannot drift between them.
    fn invoke(
        &self,
        dispatch: Value,
        paths: RArray,
        snippets: RArray,
        flags: RArray,
        entry: Entry<'_>,
    ) -> Result<Snapshot, MagnusError> {
        let ruby = Ruby::get().expect("Ruby thread");
        let verb = match entry {
            Entry::Eval { .. } => "eval",
            Entry::Run { .. } => "run",
        };
        let handler = build_handler(dispatch);
        let preamble = preamble_from(&ruby, paths.to_vec()?, snippets, flags)?;
        // Release the GVL around the guest span iff this Sandbox asks for it;
        // the closure touches no Ruby VALUE (the driver is magnus-free, and a
        // guest→host dispatch re-acquires the GVL through the bridge).
        let result = gvl::region(self.release_gvl, || {
            self.driver.invoke(entry, &preamble, handler)
        });
        result
            .map(|snapshot| Snapshot(snapshot, verb))
            .map_err(|e| errors::to_magnus(&ruby, e))
    }

    /// The bytecode of the source snippets, one binary String each in
    /// order, or `nil` when the guest compiled nothing; no failure raises,
    /// since the Sandbox keeps the source either way. Takes the Preamble an
    /// invocation sends, `paths` included, so the driver can tell whether
    /// the bytecode still fits it; no dispatch handler is bound, since
    /// compiling runs no script.
    fn compile(
        &self,
        paths: RArray,
        snippets: RArray,
        flags: RArray,
    ) -> Result<Option<RArray>, MagnusError> {
        let ruby = Ruby::get().expect("Ruby thread");
        let preamble = preamble_from(&ruby, paths.to_vec()?, snippets, flags)?;
        let bytecodes = gvl::region(self.release_gvl, || self.driver.compile(&preamble));
        Ok(bytecodes.map(|bytecodes| {
            ruby.ary_from_iter(
                bytecodes
                    .iter()
                    .map(|bytecode| ruby.str_from_slice(bytecode)),
            )
        }))
    }

    /// The driver's declaration, which the Sandbox checks against the
    /// posture its `profile:` option requested.
    fn profile(&self) -> Symbol {
        let ruby = Ruby::get().expect("Ruby thread");
        match self.driver.profile() {
            Profile::Hermetic => ruby.to_symbol("hermetic"),
            Profile::Permissive => ruby.to_symbol("permissive"),
        }
    }
}

/// The Proc stays GC-rooted for the synchronous `#eval` / `#run` call as a
/// live method argument on the Ruby stack, so the driver only borrows it
/// (the safety contract on `kobako_runtime::runtime::Runtime`).
fn build_handler(dispatch: Value) -> Option<Arc<dyn DispatchHandler>> {
    if dispatch.is_nil() {
        return None;
    }
    Some(
        Arc::new(bridge::RubyDispatchHandler::new(Opaque::from(dispatch)))
            as Arc<dyn DispatchHandler>,
    )
}

/// One invocation's result at the Ruby boundary — the whole `Snapshot` the
/// driver produced, exposed as `Kobako::Runtime::Snapshot`. Usage and the
/// two output captures are present on every outcome, so the trap path
/// carries them just like the value path; the completion is read as either
/// the Outcome (`#outcome`) or a trap (`#trap_error`). The verb names the
/// invocation, so an unsupported one says which operation is lacking.
#[derive(TypedData)]
#[magnus(class = "Kobako::Runtime::Snapshot", free_immediately, size)]
struct Snapshot(RuntimeSnapshot, &'static str);

impl DataTypeFunctions for Snapshot {}

impl Snapshot {
    /// Already split off the core envelope, so the Ruby side maps a failure
    /// onto its error taxonomy without decoding a payload byte: the ok
    /// arm's payload, or a Panic's fields beside an empty payload. A trap
    /// has no Outcome; `#trap_error` is the discriminator there.
    fn outcome(&self) -> Result<(RString, Option<PanicFields>), MagnusError> {
        let ruby = Ruby::get().expect("Ruby thread");
        match &self.0.completion {
            Completion::Outcome(Outcome::Ok(value)) => Ok((ruby.str_from_slice(value), None)),
            Completion::Outcome(Outcome::Panic(panic)) => Ok((
                ruby.str_from_slice(&[]),
                Some((
                    panic.origin.name().to_owned(),
                    panic.error.name.clone(),
                    panic.error.message.clone(),
                    panic.error.backtrace.clone(),
                    panic.available.clone(),
                )),
            )),
            // The Guest Binary answered rather than broke, so it reads as
            // the Sandbox's failure, worded by the host.
            Completion::Outcome(Outcome::Unsupported(_)) => Ok((
                ruby.str_from_slice(&[]),
                Some((
                    "sandbox".to_owned(),
                    "Kobako::SandboxError".to_owned(),
                    format!(
                        "the Sandbox runtime does not support #{}; \
                         rebuild it against the kobako version in use",
                        self.1
                    ),
                    Vec::new(),
                    Vec::new(),
                )),
            )),
            Completion::Trap(_) => Err(MagnusError::new(
                ruby.exception_runtime_error(),
                "a trapped run has no outcome",
            )),
        }
    }

    /// Unraised, so the Sandbox layer can attach the run's Execution before
    /// raising it.
    fn trap_error(&self) -> Result<Option<Exception>, MagnusError> {
        let Completion::Trap(trap) = &self.0.completion else {
            return Ok(None);
        };
        let ruby = Ruby::get().expect("Ruby thread");
        errors::trap_class(&ruby, trap)
            .new_instance((trap.to_string(),))
            .map(Some)
    }

    fn wall_time(&self) -> f64 {
        self.0.usage.wall_time
    }

    fn memory_peak(&self) -> usize {
        self.0.usage.memory_peak
    }

    fn stdout(&self) -> RString {
        let ruby = Ruby::get().expect("Ruby thread");
        ruby.str_from_slice(&self.0.stdout.bytes)
    }

    fn stdout_truncated(&self) -> bool {
        self.0.stdout.truncated
    }

    fn stderr(&self) -> RString {
        let ruby = Ruby::get().expect("Ruby thread");
        ruby.str_from_slice(&self.0.stderr.bytes)
    }

    fn stderr_truncated(&self) -> bool {
        self.0.stderr.truncated
    }
}
