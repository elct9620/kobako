//! `__kobako_run` — entrypoint dispatch entry.
//!
//! `(env_ptr, env_len)` locate the host-supplied Run envelope on linear
//! memory. Frames read from stdin: Frame 1 preamble + Frame 3 snippets
//! (docs/wire-codec.md § Invocation channels). Frame 2 is the `#eval`
//! user source and never reaches here — the entrypoint is already
//! resident as a top-level constant a preloaded snippet contributed.
//!
//! Body sequence:
//!
//! 1. Read preamble + snippets; init mrb; install kobako runtime +
//!    preamble proxy classes; replay snippets. Any failure writes a Panic envelope
//!    with the snippet's backtrace attribution
//!    and returns.
//! 2. Decode the Run envelope from `(env_ptr, env_len)`, then its
//!    payload through the codec. Either failure writes a Panic
//!    envelope.
//! 3. Resolve the entrypoint Symbol against top-level `Object` via
//!    `sys::mrb_const_defined` and confirm the constant
//!    responds to `:call` via `sys::mrb_respond_to`. Each failure writes
//!    a Panic envelope directly, naming the host class the host raises:
//!    `Kobako::UndefinedEntrypointError` for an unresolved name, which
//!    also carries the names it could have been, and
//!    `Kobako::SandboxError` for a constant that answers no `:call`.
//! 4. Invoke `target.call(*args, **kwargs)` through `mrb_funcall_argv`
//!    by concatenating the decoded args Array and (when non-empty)
//!    appending the kwargs Hash as the trailing element — the same
//!    layout `Method#call` uses internally. Serialize the return
//!    value as an ok Outcome or convert the pending mruby
//!    exception into a Panic Outcome.

/// Invocation entry behind the `__kobako_run` export — see module
/// docs. `G` supplies the shell-chosen gem set via
/// `MrbGuest::init_gems`.
#[cfg(mruby_linked)]
pub(crate) fn run<G: crate::MrbGuest>(env: &[u8]) {
    run_body::<G>(env);
}

#[cfg(mruby_linked)]
fn run_body<G: crate::MrbGuest>(env: &[u8]) {
    use super::boot;
    use crate::codec::PayloadCodec;
    use kobako_core::abi::write_panic;
    use kobako_transport::envelope::{ErrorRecord, Origin, Panic, Run};

    let preamble = match boot::read_preamble() {
        Ok(p) => p,
        Err(panic) => return write_panic(panic),
    };
    let snippets = match boot::read_snippets() {
        Ok(s) => s,
        Err(panic) => return write_panic(panic),
    };

    let kobako = match boot::acquire_vm::<G>() {
        Ok(k) => k,
        Err(panic) => return write_panic(panic),
    };
    let mrb = kobako.mrb();

    if let Err(panic) = boot::install_preamble(&kobako, &preamble) {
        return write_panic(panic);
    }

    if let Err(panic) = boot::replay_snippets(&kobako, &snippets) {
        return write_panic(panic);
    }

    // Wire faults reject here, before entrypoint resolution: a request
    // that is both malformed and aimed at a missing entrypoint reports
    // its wire-shape violation, not the entrypoint miss. The two layers
    // report separately, so a caller can tell a framing desync from a
    // payload the codec could not read.
    let run = match Run::decode(env) {
        Ok(run) => run,
        Err(_) => {
            return write_panic(boot::transport_panic(
                "failed to decode the invocation request",
            ));
        }
    };
    let arguments = match G::Codec::decode_run_arguments(&kobako, &run.payload) {
        Ok(arguments) => arguments,
        Err(err) => {
            let refusal = crate::refusal::at(crate::refusal::Position::RunArguments, err);
            return write_panic(boot::panic_for(&refusal));
        }
    };

    // Resolve entrypoint Symbol against top-level `Object`. The whole
    // dispatch — const lookup, `respond_to?(:call)` gate,
    // and the `target.call(*args, **kwargs)` invocation —
    // runs through the mruby C API. No Ruby trampoline, no global
    // variable injection.
    let target_sym = mrb.intern_str(mrb.str_new(run.entrypoint.as_bytes()).as_value());
    // SAFETY: the cached `object_class` pointer was produced by the
    // same `mrb_state` and is GC-stable for the VM's lifetime.
    let object_value = unsafe { mrb.object_class().to_value(mrb) };

    if !object_value.const_defined(mrb, target_sym) {
        let available = super::boot_constants::snippet_constants(&kobako, &preamble);
        return write_panic(Panic {
            origin: Origin::Sandbox,
            error: ErrorRecord {
                name: "Kobako::UndefinedEntrypointError".into(),
                message: format!("undefined entrypoint: {}", run.entrypoint),
                backtrace: Vec::new(),
            },
            available,
        });
    }

    let target_val = match object_value.const_get(mrb, target_sym) {
        Ok(v) => v,
        // The `const_defined` gate above makes a plain undefined-constant
        // miss unreachable here; a surfaced error is the exotic case
        // (e.g. an autoload hook raised). Attribute it verbatim rather
        // than silently swallow it.
        Err(err) => return write_panic(boot::panic_from_error(&kobako, err)),
    };

    let call_sym = mrb.intern_cstr(c"call");
    if !target_val.respond_to(mrb, call_sym) {
        return write_panic(Panic {
            origin: Origin::Sandbox,
            error: ErrorRecord {
                name: "Kobako::SandboxError".into(),
                message: format!("entrypoint {} does not respond to :call", run.entrypoint),
                backtrace: Vec::new(),
            },
            available: Vec::new(),
        });
    }

    // Build argv = [*args, kwargs?] where the trailing kwargs Hash is
    // appended as a positional argument (omitted when empty so a
    // `def call(*a)` entrypoint does not see an unwanted Hash tail).
    //
    // mruby C API limitation: `mrb_funcall_argv` and the entire
    // `mrb_funcall_*` family force `ci->nk = 0` on entry ("funcall does
    // not support keyword arguments"), so callers cannot mark the
    // trailing Hash as a kwargs splat. Entrypoints therefore see kwargs
    // as the last
    // positional argument and must accept it as a plain `Hash` (e.g.
    // `def call(req, opts = {})` rather than `def call(req,
    // multiplier: 1)`).
    // An argument the guest cannot represent — an integer outside the
    // 32-bit range — fails the invocation rather than reaching the
    // entrypoint with a saturated value (docs/wire/payload-msgpack.md § Integer
    // Range).
    let mut argv = arguments.args;
    if let Some(kwargs) = arguments.kwargs {
        argv.push(kwargs.as_value());
    }

    let result_val = match target_val.funcall_argv(mrb, call_sym, &argv) {
        Ok(v) => v,
        Err(err) => return write_panic(boot::panic_from_error(&kobako, err)),
    };

    boot::write_value_outcome::<G>(&kobako, result_val);
}
