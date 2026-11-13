//! The Ruby handle of the contract `Runtime` a Sandbox runs each
//! invocation on.
//!
//! The Ruby-visible classes are
//!
//!   Kobako::Runtime  — holds one contract `Runtime`
//!   Kobako::Snapshot — one invocation's completion + captures + usage
//!
//! `Kobako::Runtime` is constructed via `Kobako::Runtime.from_path(path,
//! **options)`, the options keyed by the `Kobako::SandboxOptions` members. Every
//! invocation (`#eval` / `#run`) takes that run's dispatch handler as a call
//! argument, instantiates a fresh instance, and returns a `Snapshot` — the
//! whole per-invocation result — so the Runtime holds no per-invocation
//! state and one Runtime is safe to drive concurrently. The run mechanics —
//! engine/module caches, caps, trap classification — live in the Wasm
//! Driver `from_path` builds, behind the `kobako_runtime` contract; no wasm
//! engine type reaches this crate or the Host App.
//!
//! Each module holds one responsibility and opens with its own doc.

mod bridge;
mod snapshot;

use magnus::{
    function, method, prelude::*, scan_args::get_kwargs, typed_data::DataTypeFunctions,
    value::Opaque, Error as MagnusError, RArray, RHash, RModule, RString, Ruby, Symbol, TypedData,
    Value,
};

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use kobako_runtime::dispatch::DispatchHandler;
use kobako_runtime::error::SetupError;
use kobako_runtime::profile::Profile;
use kobako_runtime::runtime::{Entry, Runtime as ContractRuntime};
use kobako_transport::envelope::{Bindings, Flag, Preamble, Snippet, Snippets};
use kobako_wasmtime::{Config, Driver};

use crate::{errors, escape, filesystem, gvl, network, rstring_to_vec, symbol_name};
use snapshot::Snapshot;

/// `kind` arrives as a Symbol so the registry never names the wire's
/// discriminant; an unknown kind raises rather than defaulting.
fn to_snippets(ruby: &Ruby, entries: RArray) -> Result<Snippets, MagnusError> {
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
/// and a compile cannot drift in what they send. An invocation's setup
/// and a compile's preamble both open with its paths, snippets, and flags.
fn to_preamble(ruby: &Ruby, parts: RArray) -> Result<Preamble, MagnusError> {
    let paths: RArray = parts.entry(0)?;
    let flags: RArray = parts.entry(2)?;
    let mut preamble = Preamble::default();
    preamble.bindings = Bindings {
        paths: paths.to_vec()?,
    };
    preamble.snippets = to_snippets(ruby, parts.entry(1)?)?;
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

pub(crate) fn register(ruby: &Ruby, kobako: RModule) -> Result<(), MagnusError> {
    let runtime = kobako.define_class("Runtime", ruby.class_object())?;
    runtime.define_singleton_method("from_path", function!(Runtime::from_path, 2))?;
    runtime.define_singleton_method("default_options", function!(Runtime::default_options, 0))?;
    runtime.define_method("eval", method!(Runtime::eval, 3))?;
    runtime.define_method("run", method!(Runtime::run, 4))?;
    runtime.define_method("profile", method!(Runtime::profile, 0))?;
    runtime.define_method("compile", method!(Runtime::compile, 1))?;
    // The guest re-enters for a block yield through a frame-scoped
    // `Kobako::GuestYielder` the dispatcher hands the Proc, not a method on
    // Runtime.
    bridge::register(kobako)?;

    // Snapshot — the per-invocation result object each entry point returns.
    snapshot::register(ruby, kobako)?;

    // A wrapped class is otherwise looked up the first time an object is
    // wrapped, through a Ruby call that a pending interrupt would turn into
    // a panic; a signal arriving during a process's first run waits for
    // exactly that moment.
    Runtime::class(ruby);
    Snapshot::class(ruby);

    Ok(())
}

#[derive(TypedData)]
#[magnus(class = "Kobako::Runtime", free_immediately, size)]
struct Runtime {
    // The runtime every invocation runs on, held behind the contract so the
    // driver is named only where it is built. The Runtime holds no
    // per-invocation state — each `#eval` / `#run` takes its dispatch
    // handler as an argument and returns its whole result as a `Snapshot`
    // — so the contract's `Send + Sync` carries the type with no interior
    // mutability to guard.
    runtime: Box<dyn ContractRuntime + Send + Sync>,
    // Whether each invocation releases Ruby's GVL for its guest span
    // (`gvl: :release`) or holds it throughout (`gvl: :hold`). Fixed at
    // construction; a `bool` carries no interior mutability, so the type
    // stays `Send + Sync`.
    release_gvl: bool,
}

impl DataTypeFunctions for Runtime {}

impl Runtime {
    /// Each option `from_path` takes, keyed as `Kobako::SandboxOptions` names
    /// it, at the driver's default, so every frontend applies one set. A
    /// run holds the GVL unless the Host App asks to release it.
    fn default_options() -> Result<RHash, MagnusError> {
        let ruby = Ruby::get().expect("Ruby thread");
        let config = Config::default();
        let options = ruby.hash_new();
        let seconds = |deadline: Option<Duration>| deadline.map(|d| d.as_secs_f64());
        options.aset(ruby.to_symbol("timeout"), seconds(config.timeout))?;
        options.aset(
            ruby.to_symbol("setup_timeout"),
            seconds(config.setup_timeout),
        )?;
        options.aset(ruby.to_symbol("memory_limit"), config.memory_limit)?;
        options.aset(ruby.to_symbol("stdout_limit"), config.stdout_limit)?;
        options.aset(ruby.to_symbol("stderr_limit"), config.stderr_limit)?;
        options.aset(ruby.to_symbol("open_file_limit"), config.open_file_limit)?;
        options.aset(
            ruby.to_symbol("open_connection_limit"),
            config.open_connection_limit,
        )?;
        options.aset(ruby.to_symbol("connection_limit"), config.connection_limit)?;
        options.aset(
            ruby.to_symbol("profile"),
            profile_name(&ruby, config.profile),
        )?;
        options.aset(ruby.to_symbol("gvl"), ruby.to_symbol("hold"))?;
        Ok(options)
    }

    /// The only Ruby-facing constructor, so Engine and Module are never
    /// visible to Ruby. Every option is a required keyword, as
    /// `default_options` answers each.
    fn from_path(path: String, options: RHash) -> Result<Self, MagnusError> {
        let ruby = Ruby::get().expect("Ruby thread");
        // `get_kwargs` scans at most eight required keywords, so the two
        // connection caps are taken out first, each still required.
        let take_cap = |name: &str| -> Result<Option<usize>, MagnusError> {
            let key = ruby.to_symbol(name);
            let cap = options.fetch(key)?;
            options.delete::<_, Value>(key)?;
            Ok(cap)
        };
        let open_connection_limit = take_cap("open_connection_limit")?;
        let connection_limit = take_cap("connection_limit")?;
        type Options = (
            Option<f64>,
            Option<f64>,
            Option<usize>,
            Option<usize>,
            Option<usize>,
            Option<usize>,
            Value,
            Value,
        );
        let options = get_kwargs::<_, Options, (), ()>(
            options,
            &[
                "timeout",
                "setup_timeout",
                "memory_limit",
                "stdout_limit",
                "stderr_limit",
                "open_file_limit",
                "profile",
                "gvl",
            ],
            &[],
        )?;
        let (
            timeout_seconds,
            setup_timeout_seconds,
            memory_limit,
            stdout_limit,
            stderr_limit,
            open_file_limit,
            profile,
            gvl,
        ) = options.required;
        // `SandboxOptions` hands over each deadline already positive and
        // finite.
        let timeout = timeout_seconds.map(Duration::from_secs_f64);
        let setup_timeout = setup_timeout_seconds.map(Duration::from_secs_f64);
        // A value kobako does not name a profile has no profile to stand
        // for, so it is refused rather than read as a weaker one.
        let profile = match symbol_name(profile)?.as_deref() {
            Some("hermetic") => Profile::Hermetic,
            Some("permissive") => Profile::Permissive,
            _ => {
                return Err(MagnusError::new(
                    ruby.exception_arg_error(),
                    format!(
                        "profile must be :permissive or :hermetic, got {}",
                        profile.inspect()
                    ),
                ));
            }
        };
        // A mode kobako does not name is refused as a profile is.
        let release_gvl = match symbol_name(gvl)?.as_deref() {
            Some("hold") => false,
            Some("release") => true,
            _ => {
                return Err(MagnusError::new(
                    ruby.exception_arg_error(),
                    format!("gvl must be :hold or :release, got {}", gvl.inspect()),
                ));
            }
        };

        let mut config = Config::default();
        config.timeout = timeout;
        config.setup_timeout = setup_timeout;
        config.memory_limit = memory_limit;
        config.stdout_limit = stdout_limit;
        config.stderr_limit = stderr_limit;
        config.open_file_limit = open_file_limit;
        config.open_connection_limit = open_connection_limit;
        config.connection_limit = connection_limit;
        config.profile = profile;
        let driver =
            Driver::new(Path::new(&path), config).map_err(|e| errors::setup_to_magnus(&ruby, e))?;
        // The requested profile is a floor: a runtime that built less never
        // runs untrusted code under less isolation than was asked for.
        let declared = driver.profile();
        if declared < profile {
            return Err(errors::setup_to_magnus(
                &ruby,
                SetupError::Dead(format!(
                    "runtime declares isolation profile {}, below the requested floor {}",
                    profile_name(&ruby, declared).inspect(),
                    profile_name(&ruby, profile).inspect()
                )),
            ));
        }
        Ok(Self {
            runtime: Box::new(driver),
            release_gvl,
        })
    }

    // -----------------------------------------------------------------
    // Run-path methods. Each takes the run's dispatch handler as its first
    // argument and returns a `Snapshot` for any completed invocation —
    // success or trap alike. Only a could-not-start fault (a missing export
    // or a fault before the export call) raises directly, as the class
    // `errors::to_magnus` gives its cause, since it yields no `Snapshot`.
    //
    // `setup` is the invocation's `[paths, snippets, flags, mounts,
    // networks]`: the Preamble's three parts, then the Mounts as
    // `filesystem::to_grants` reads them and the network entries as
    // `network::to_grants` reads them.
    // -----------------------------------------------------------------

    fn eval(
        &self,
        dispatch: Value,
        setup: RArray,
        source: RString,
    ) -> Result<Snapshot, MagnusError> {
        let source = rstring_to_vec(source);
        self.invoke(dispatch, setup, Entry::Eval { source: &source })
    }

    fn run(
        &self,
        dispatch: Value,
        setup: RArray,
        entrypoint: String,
        payload: RString,
    ) -> Result<Snapshot, MagnusError> {
        self.invoke(
            dispatch,
            setup,
            Entry::Run {
                entrypoint: &entrypoint,
                payload: rstring_to_vec(payload),
            },
        )
    }

    /// One owner for the wiring both verbs share, so a Preamble, a Grant,
    /// or a handler change cannot drift between them.
    fn invoke(
        &self,
        dispatch: Value,
        setup: RArray,
        entry: Entry<'_>,
    ) -> Result<Snapshot, MagnusError> {
        let ruby = Ruby::get().expect("Ruby thread");
        let verb = match entry {
            Entry::Eval { .. } => "eval",
            Entry::Run { .. } => "run",
        };
        let handler = build_handler(dispatch);
        let preamble = to_preamble(&ruby, setup)?;
        let (mut grants, mut pins) = filesystem::to_grants(&ruby, setup.entry(3)?)?;
        let (networks, network_pins) = network::to_grants(&ruby, setup.entry(4)?)?;
        grants.extend(networks);
        pins.extend(network_pins);
        // Release the GVL around the guest span iff this Sandbox asks for it;
        // the closure touches no Ruby VALUE (the runtime is magnus-free, and a
        // guest→host dispatch, or a mount's block, re-acquires the GVL).
        let result = gvl::region(self.release_gvl, || {
            self.runtime.invoke(entry, &preamble, handler, &grants)
        });
        escape::resume()?;
        result
            .map(|snapshot| Snapshot::new(snapshot, verb))
            .map_err(|e| errors::to_magnus(&ruby, e))
    }

    /// The bytecode of the source snippets, one binary String each in
    /// order, or `nil` when the guest compiled nothing; no failure raises,
    /// since the Sandbox keeps the source either way. Takes the Preamble an
    /// invocation sends, `paths` included, so the runtime can tell whether
    /// the bytecode still fits it; no dispatch handler is bound, since
    /// compiling runs no script.
    fn compile(&self, preamble: RArray) -> Result<Option<RArray>, MagnusError> {
        let ruby = Ruby::get().expect("Ruby thread");
        let preamble = to_preamble(&ruby, preamble)?;
        let bytecodes = gvl::region(self.release_gvl, || self.runtime.compile(&preamble));
        escape::resume()?;
        Ok(bytecodes.map(|bytecodes| {
            ruby.ary_from_iter(
                bytecodes
                    .iter()
                    .map(|bytecode| ruby.str_from_slice(bytecode)),
            )
        }))
    }

    /// The runtime's declaration, which the Sandbox checks against the
    /// profile its `profile:` option requested.
    fn profile(&self) -> Symbol {
        let ruby = Ruby::get().expect("Ruby thread");
        profile_name(&ruby, self.runtime.profile())
    }
}

/// `profile` as the Symbol the `profile:` option names it by.
fn profile_name(ruby: &Ruby, profile: Profile) -> Symbol {
    ruby.to_symbol(match profile {
        Profile::Hermetic => "hermetic",
        Profile::Permissive => "permissive",
    })
}

/// The Proc stays GC-rooted for the synchronous `#eval` / `#run` call as a
/// live method argument on the Ruby stack, so the runtime only borrows it
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
