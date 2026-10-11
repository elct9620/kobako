//! `__kobako_run` — entrypoint dispatch entry.
//!
//! `export_guest!` has already read the Frames and the host-supplied
//! Run envelope, so this flow starts from their fields. The entrypoint
//! is already resident as a top-level constant a preloaded snippet
//! contributed.
//!
//! Body sequence:
//!
//! 1. Init mrb; install kobako runtime + the bound paths' proxy
//!    classes; replay snippets. Any failure answers a Panic Outcome
//!    with the snippet's backtrace attribution.
//! 2. Decode the Run's payload through the codec. A failure answers a
//!    Panic Outcome.
//! 3. Resolve the entrypoint Symbol against top-level `Object` via
//!    `sys::mrb_const_defined` and confirm the constant
//!    responds to `:call` via `sys::mrb_respond_to`. Each failure answers
//!    a Panic Outcome directly, naming the host class the host raises:
//!    `Kobako::UndefinedEntrypointError` for an unresolved name, which
//!    also carries the names it could have been, and
//!    `Kobako::SandboxError` for a constant that answers no `:call`.
//! 4. Invoke `target.call(*args, **kwargs)` through `mrb_funcall_argv`
//!    by concatenating the decoded args Array and (when non-empty)
//!    appending the kwargs Hash as the trailing element — the same
//!    layout `Method#call` uses internally. Serialize the return
//!    value as an ok Outcome or convert the raised exception into a
//!    Panic Outcome.

use kobako_transport::envelope::{Frames, Outcome, Panic, Run};

pub(crate) fn run<G: crate::MrbGuest>(frames: Frames, run: Run) -> Outcome {
    call_entrypoint::<G>(&frames, run).unwrap_or_else(Outcome::Panic)
}

fn call_entrypoint<G: crate::MrbGuest>(frames: &Frames, run: Run) -> Result<Outcome, Panic> {
    use super::{boot, panic};
    use crate::codec::PayloadCodec;
    use beni::{Module, ReprValue};

    let kobako = boot::enter::<G>(frames)?;
    let mrb = kobako.mrb();

    // A payload the codec cannot read rejects here, before entrypoint
    // resolution: a request that is both unreadable and aimed at a
    // missing entrypoint reports the payload, not the entrypoint miss.
    let arguments = G::Codec::decode_run_arguments(&kobako, &run.payload).map_err(|err| {
        panic::panic_for(&crate::refusal::at(
            crate::refusal::Position::RunArguments,
            err,
        ))
    })?;

    // Resolve entrypoint Symbol against top-level `Object`. The whole
    // dispatch — const lookup, `respond_to?(:call)` gate,
    // and the `target.call(*args, **kwargs)` invocation —
    // runs through the mruby C API. No Ruby trampoline, no global
    // variable injection.
    // Looked up without interning: a constant is named by a symbol that
    // already exists, so a name with none is undefined without adding one
    // to the guest's symbol table.
    let object_class = mrb.object_class();
    let defined = mrb
        .check_id(run.entrypoint.as_bytes())
        .filter(|&sym| object_class.const_defined(mrb, sym));
    let Some(target_sym) = defined else {
        let available = super::boot_constants::snippet_constants(&kobako, &frames.bindings.paths);
        return Err(Panic {
            available,
            ..panic::sandbox_panic(
                "Kobako::UndefinedEntrypointError",
                format!("undefined entrypoint: {}", run.entrypoint),
            )
        });
    };

    // The `const_defined` gate above makes a plain undefined-constant
    // miss unreachable here; a surfaced error is the exotic case (e.g.
    // an autoload hook raised). Attribute it verbatim rather than
    // silently swallow it.
    let target_val = object_class
        .const_get::<_, beni::Value>(mrb, target_sym)
        .map_err(|err| panic::panic_from_error(&kobako, err))?;

    if !target_val.respond_to(mrb, c"call") {
        return Err(panic::flow_panic(format!(
            "entrypoint {} does not respond to :call",
            run.entrypoint
        )));
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

    let result_val = target_val
        .funcall(mrb, c"call", &argv)
        .map_err(|err| panic::panic_from_error(&kobako, err))?;

    Ok(panic::value_outcome::<G>(&kobako, result_val))
}
