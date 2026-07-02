// Host-side magnus shell over the extracted wasmtime driver.
//
// The only Ruby-visible class is
//
//   Kobako::Runtime — wraps a `kobako_wasmtime::Driver` + the Ruby seams
//
// constructed via `Kobako::Runtime.from_path(path, timeout, memory_limit,
// stdout_limit, stderr_limit)`. Every invocation (`#eval` / `#run`)
// instantiates a fresh instance and discards the whole Store afterwards —
// the per-invocation instance discipline (ABI v2). The run mechanics —
// engine/module caches, caps, trap classification — live in the
// `kobako-wasmtime` crate behind the `kobako_runtime` contract; no wasm
// engine type reaches this crate or Ruby (SPEC.md "Code Organization":
// `ext/` "exposes no Wasm engine types to the Host App or downstream
// gems").
//
// Module layout (per CLAUDE.md principle #2 — one responsibility per file):
//
//   * `bridge`      — the magnus dispatch bridge: `RubyDispatchHandler` +
//                     the frame-scoped `GuestYielder` Ruby class.
//   * `errors`      — the single boundary mapping the neutral `Trap` /
//                     `SetupError` channels onto the `Kobako::*` classes.
//
// This file owns the `Kobako::Runtime` magnus class itself — the Ruby
// init() that registers the class, the byte↔`RString` shuttling, the
// dispatch-Proc GC root, and the per-invocation usage readout.

mod bridge;
mod errors;

use magnus::{function, method, prelude::*, Error as MagnusError, RModule, RString, Ruby};

use std::cell::Cell;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use magnus::{gc, typed_data::DataTypeFunctions, value::Opaque, RArray, TypedData, Value};

use crate::snapshot::Snapshot;
use kobako_runtime::dispatch::DispatchHandler;
use kobako_runtime::runtime::{Entry, Frames, Runtime as ContractRuntime};
use kobako_runtime::snapshot::{Completion, Snapshot as RuntimeSnapshot, Usage};
use kobako_wasmtime::{Config, Driver};

/// Copy the bytes of `s` into a fresh `Vec<u8>`. Single safe entry to
/// what would otherwise be an inline `unsafe { rstring.as_slice() }
/// .to_vec()` duplicated at every host-↔-guest boundary. The borrow
/// does not outlive this call, so no Ruby allocation can move the
/// underlying RString between the borrow and the copy — the safety
/// invariant the inline form relied on is established once here.
fn rstring_to_vec(s: RString) -> Vec<u8> {
    // SAFETY: see item doc.
    unsafe { s.as_slice() }.to_vec()
}

// ---------------------------------------------------------------------------
// Ruby init
// ---------------------------------------------------------------------------

pub fn init(ruby: &Ruby, kobako: RModule) -> Result<(), MagnusError> {
    // Error hierarchy lives in `lib/kobako/errors.rb` (top-level
    // `Kobako::TrapError` / `TimeoutError` / `MemoryLimitError` /
    // `SetupError` / `ModuleNotBuiltError`). The ext raises directly into
    // those classes through `trap_err` / `timeout_err` / `memory_limit_err`
    // / `sandbox_err` / `setup_err` / `MODULE_NOT_BUILT_ERROR`; no
    // intermediate hierarchy is registered.

    let runtime = kobako.define_class("Runtime", ruby.class_object())?;
    runtime.define_singleton_method("from_path", function!(Runtime::from_path, 5))?;
    runtime.define_method("on_dispatch=", method!(Runtime::set_on_dispatch, 1))?;
    runtime.define_method("eval", method!(Runtime::eval, 3))?;
    runtime.define_method("run", method!(Runtime::run, 3))?;
    runtime.define_method("usage", method!(Runtime::usage, 0))?;
    // The guest re-enters for a block yield through a frame-scoped
    // `Kobako::Runtime::GuestYielder` the dispatcher hands the Proc, not a
    // method on Runtime.
    bridge::register(runtime)?;

    Ok(())
}

#[derive(TypedData)]
#[magnus(class = "Kobako::Runtime", free_immediately, size, mark)]
struct Runtime {
    // The magnus-free wasmtime driver that runs every invocation; the
    // shell only shuttles Ruby values across its boundary.
    driver: Driver,
    // The host-side dispatch Proc, held here only
    // to give `DataTypeFunctions::mark` a read path so it can pin the
    // Proc across GC. For each invocation `build_handler` wraps a copy of
    // this handle in a `RubyDispatchHandler`, and the driver's `invoke`
    // binds that `Arc<dyn DispatchHandler>` onto the per-invocation
    // `Invocation`, where the `__kobako_dispatch` import calls it — both
    // reference the one Proc this `Opaque` pins. `Cell` is sound under the
    // GVL (see the `unsafe impl Sync` below).
    on_dispatch: Cell<Option<Opaque<Value>>>,
    // Usage of the most recent invocation, kept apart from the returned
    // `Snapshot` so `#usage` reads survive the per-invocation Store
    // teardown and the trap path's raise. Zeroed before the first
    // invocation.
    last_usage: Cell<Usage>,
}

impl DataTypeFunctions for Runtime {
    /// Mark — and thereby pin — the host-side dispatch Proc so Ruby's GC
    /// neither collects nor moves it while the ext holds a raw `Opaque`
    /// copy on `Invocation` for the duration of a guest invocation.
    /// `gc::Marker::mark` maps to `rb_gc_mark`, which pins: required because
    /// the Invocation copy is a cached `VALUE` that compaction would
    /// otherwise leave dangling. Without
    /// this the Proc has no GC root at all — sweep collects it (SIGSEGV on
    /// the next dispatch) and compaction relocates it (dispatch lands on
    /// the wrong receiver).
    fn mark(&self, marker: &gc::Marker) {
        if let Some(on_dispatch) = self.on_dispatch.get() {
            marker.mark(on_dispatch);
        }
    }
}

// SAFETY: magnus requires `Send + Sync` on TypedData types. The
// `on_dispatch` / `last_usage` `Cell`s make the auto-derived `Sync`
// unavailable, but every access to them happens under the GVL on a single
// thread at a time — Ruby method calls, and a GC `mark` pass that also
// holds the GVL. No cross-thread access to either Cell can occur. `Send`
// stays auto-derived.
unsafe impl Sync for Runtime {}

impl Runtime {
    /// Construct a Runtime from a wasm file path, using the process-wide
    /// shared Engine and per-path Module / InstancePre caches. The single
    /// Ruby-facing constructor for `Kobako::Runtime` — Engine and Module
    /// are never visible to Ruby.
    ///
    /// `timeout_seconds` is the wall-clock cap in seconds
    /// (`None` disables); `memory_limit` is the linear-memory cap in
    /// bytes (`None` disables); `stdout_limit_bytes` / `stderr_limit_bytes`
    /// are the per-channel output caps (`None`
    /// disables). All four are validated by the caller
    /// (`Kobako::Sandbox`); this method only refuses non-finite or
    /// non-positive timeouts as a defence in depth.
    fn from_path(
        path: String,
        timeout_seconds: Option<f64>,
        memory_limit: Option<usize>,
        stdout_limit_bytes: Option<usize>,
        stderr_limit_bytes: Option<usize>,
    ) -> Result<Self, MagnusError> {
        let ruby = Ruby::get().expect("Ruby thread");
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

        let driver = Driver::new(
            Path::new(&path),
            memory_limit,
            Config {
                timeout,
                stdout_limit_bytes,
                stderr_limit_bytes,
            },
        )
        .map_err(|e| errors::setup_to_magnus(&ruby, e))?;
        Ok(Self {
            driver,
            on_dispatch: Cell::new(None),
            last_usage: Cell::new(Usage {
                wall_time: 0.0,
                memory_peak: 0,
            }),
        })
    }

    /// Register the Ruby-side dispatch `Proc`.
    /// Bound to Ruby as `Kobako::Runtime#on_dispatch=`. The handle is
    /// pinned by `DataTypeFunctions::mark`; for each invocation
    /// `build_handler` wraps a copy in a `RubyDispatchHandler` and the
    /// driver's `invoke` binds it onto the per-invocation `Invocation`,
    /// where the `__kobako_dispatch` import reads it through
    /// `Caller<Invocation>`.
    fn set_on_dispatch(&self, proc_value: Value) -> Result<(), MagnusError> {
        self.on_dispatch.set(Some(Opaque::from(proc_value)));
        Ok(())
    }

    // -----------------------------------------------------------------
    // Run-path methods. Each method is best-effort — it raises a Ruby
    // `Kobako::TrapError` when the corresponding export is missing or
    // fails so the Sandbox layer can map errors to the three-class
    // taxonomy.
    // -----------------------------------------------------------------

    /// One-shot mruby source execution (`#eval`). The Ruby-facing entry:
    /// builds the dispatch handler from the registered Proc, hands the
    /// three stdin frames (`preamble`, `source`, `snippets`) and the source
    /// to the driver, and settles the returned `Snapshot` through
    /// `finish_invocation` — or maps a could-not-start `Error` onto its
    /// `Kobako::*` exception. The run mechanics — frames, caps, trap
    /// classification — live in `kobako_wasmtime::Driver`.
    fn eval(
        &self,
        preamble: RString,
        source: RString,
        snippets: RString,
    ) -> Result<Snapshot, MagnusError> {
        let ruby = Ruby::get().expect("Ruby thread");
        let handler = self.build_handler();
        let preamble = rstring_to_vec(preamble);
        let source = rstring_to_vec(source);
        let snippets = rstring_to_vec(snippets);
        let snapshot = self
            .driver
            .invoke(
                Entry::Eval { source: &source },
                Frames {
                    preamble: &preamble,
                    snippets: &snippets,
                },
                handler,
            )
            .map_err(|e| errors::to_magnus(&ruby, e))?;
        self.finish_invocation(&ruby, snapshot)
    }

    /// Execute one entrypoint dispatch (`__kobako_run`) and return a
    /// `Snapshot` bundling every per-invocation observable.
    ///
    /// The two-frame stdin protocol (preamble + snippets; no user source
    /// frame — docs/wire-codec.md § Invocation channels) plus the
    /// `envelope` copied into guest linear memory; cap semantics match
    /// `#eval`. Raises `Kobako::TrapError` / `Kobako::SandboxError` per the
    /// engine-vs-host-fault split inside the driver.
    fn run(
        &self,
        preamble: RString,
        snippets: RString,
        envelope: RString,
    ) -> Result<Snapshot, MagnusError> {
        let ruby = Ruby::get().expect("Ruby thread");
        let handler = self.build_handler();
        let preamble = rstring_to_vec(preamble);
        let snippets = rstring_to_vec(snippets);
        let envelope = rstring_to_vec(envelope);
        let snapshot = self
            .driver
            .invoke(
                Entry::Run {
                    envelope: &envelope,
                },
                Frames {
                    preamble: &preamble,
                    snippets: &snippets,
                },
                handler,
            )
            .map_err(|e| errors::to_magnus(&ruby, e))?;
        self.finish_invocation(&ruby, snapshot)
    }

    /// Settle one invocation's `Snapshot` at the Ruby boundary: usage is
    /// recorded even when the completion is a trap and this call raises,
    /// while trap-path captures are deliberately dropped — exposing them
    /// is a SPEC decision the Ruby surface has not taken.
    fn finish_invocation(
        &self,
        ruby: &Ruby,
        snapshot: RuntimeSnapshot,
    ) -> Result<Snapshot, MagnusError> {
        self.last_usage.set(snapshot.usage);
        match snapshot.completion {
            Completion::Outcome(bytes) => {
                Ok(Snapshot::new(bytes, snapshot.stdout, snapshot.stderr))
            }
            Completion::Trap(trap) => Err(errors::trap_to_magnus(ruby, trap)),
        }
    }

    /// Build the dispatch handler for one invocation from the registered
    /// `on_dispatch` Proc, or `None` when none is set. The `Opaque` the
    /// handler wraps stays GC-rooted by `Runtime`'s `mark`, so the driver
    /// only borrows it for the call (the safety contract on
    /// `kobako_runtime::runtime::Runtime`).
    fn build_handler(&self) -> Option<Arc<dyn DispatchHandler>> {
        self.on_dispatch.get().map(|proc| {
            Arc::new(bridge::RubyDispatchHandler::new(proc)) as Arc<dyn DispatchHandler>
        })
    }

    /// Return the per-last-invocation usage as a
    /// Ruby 2-tuple `[wall_time, memory_peak]`. The element order
    /// matches the `Kobako::Usage` field order declared in
    /// `lib/kobako/usage.rb`; reorder both sides together if the field
    /// list ever grows.
    ///
    ///   * `wall_time` (Float seconds) — the wall-clock duration the
    ///     most recent invocation spent inside the guest export call.
    ///     The bracket mirrors the `timeout` deadline accounting and
    ///     excludes everything that runs after the guest export
    ///     returns. `0.0` before the first invocation.
    ///   * `memory_peak` (Integer bytes) — the high-water mark of the
    ///     per-invocation `memory.grow` delta past the linear-memory
    ///     size captured at invocation entry. `0` before the first
    ///     invocation.
    ///
    /// Reads the `last_usage` Cell `finish_invocation` populated from the
    /// returned `Snapshot` before the per-invocation Store was discarded.
    fn usage(&self) -> Result<RArray, MagnusError> {
        let ruby = Ruby::get().expect("Ruby thread");
        let usage = self.last_usage.get();
        let arr = ruby.ary_new_capa(2);
        arr.push(usage.wall_time)?;
        arr.push(usage.memory_peak)?;
        Ok(arr)
    }
}
