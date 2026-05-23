//! `__kobako_run` — entrypoint dispatch entry (docs/behavior.md B-31).
//!
//! `(env_ptr, env_len)` locate the host-supplied invocation envelope on
//! linear memory. Frames read from stdin: Frame 1 preamble + Frame 2
//! snippets only (no user-source frame — the entrypoint is already
//! resident as a top-level constant contributed by a preloaded
//! snippet).
//!
//! Body sequence:
//!
//! 1. Read preamble + snippets; init mrb; install kobako runtime +
//!    namespaces; replay snippets. Any failure writes a Panic envelope
//!    with the snippet's backtrace attribution (docs/behavior.md E-36)
//!    and returns.
//! 2. Decode the invocation envelope from `(env_ptr, env_len)` via
//!    [`parse_invocation`]. Decode failure writes a Panic envelope
//!    (E-26).
//! 3. Resolve the entrypoint Symbol against top-level `Object` via
//!    [`sys::mrb_const_defined`] (E-27) and confirm the constant
//!    responds to `:call` via [`sys::mrb_respond_to`] (E-28). Each
//!    failure writes a Panic envelope directly with the SPEC-mandated
//!    `Kobako::SandboxError` class string.
//! 4. Invoke `target.call(*args, **kwargs)` through `mrb_funcall_argv`
//!    by concatenating the decoded args Array and (when non-empty)
//!    appending the kwargs Hash as the trailing element — the same
//!    layout `Method#call` uses internally. Serialize the return
//!    value as a Result envelope or convert the pending mruby
//!    exception into a Panic envelope.

#[cfg(any(target_arch = "wasm32", test))]
use crate::codec::Value;

/// Decoded invocation envelope. `target` is the entrypoint constant
/// name (a Symbol on the codec side); `args` is always a
/// [`Value::Array`] and `kwargs` always a [`Value::Map`] — callers can
/// hand them straight to [`crate::kobako::Kobako::to_mrb_value`]
/// without re-checking.
#[cfg(any(target_arch = "wasm32", test))]
#[derive(Debug, PartialEq)]
pub(super) struct Invocation {
    pub target: String,
    pub args: Value,
    pub kwargs: Value,
}

/// Reasons the invocation envelope failed to decode. Each variant
/// carries the host-visible Panic message verbatim; the wrapper at
/// [`__kobako_run`] folds the variant back into a
/// `Kobako::Transport::WireError` Panic.
#[cfg(any(target_arch = "wasm32", test))]
#[derive(Debug, PartialEq)]
pub(super) enum InvocationError {
    /// Envelope was not a msgpack map.
    NotMap,
    /// `entrypoint` key was absent or its value was not a Symbol.
    MissingEntrypoint,
}

#[cfg(any(target_arch = "wasm32", test))]
impl InvocationError {
    pub(super) fn message(&self) -> &'static str {
        match self {
            Self::NotMap => "invocation envelope must be a msgpack map",
            Self::MissingEntrypoint => "invocation envelope missing entrypoint Symbol",
        }
    }
}

/// Parse a decoded msgpack [`Value`] into an [`Invocation`]. Unknown
/// keys are silently ignored for forward compatibility; `args` /
/// `kwargs` default to empty array / empty map when absent. Pure
/// parser — host-buildable for unit testing.
#[cfg(any(target_arch = "wasm32", test))]
pub(super) fn parse_invocation(envelope: Value) -> Result<Invocation, InvocationError> {
    let pairs = match envelope {
        Value::Map(p) => p,
        _ => return Err(InvocationError::NotMap),
    };
    let mut target: Option<String> = None;
    let mut args_val: Option<Value> = None;
    let mut kwargs_val: Option<Value> = None;
    for (k, v) in pairs {
        let key = match k {
            Value::Str(s) => s,
            _ => continue,
        };
        match key.as_str() {
            "entrypoint" => {
                if let Value::Sym(name) = v {
                    target = Some(name);
                }
            }
            "args" => args_val = Some(v),
            "kwargs" => kwargs_val = Some(v),
            _ => {}
        }
    }
    let target = target.ok_or(InvocationError::MissingEntrypoint)?;
    let args = args_val.unwrap_or(Value::Array(Vec::new()));
    let kwargs = kwargs_val.unwrap_or(Value::Map(Vec::new()));
    Ok(Invocation {
        target,
        args,
        kwargs,
    })
}

/// Reactor entry — see module docs.
#[no_mangle]
pub extern "C" fn __kobako_run(env_ptr: i32, env_len: i32) {
    #[cfg(target_arch = "wasm32")]
    {
        run_body(env_ptr, env_len);
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        // Host stub — the reactor export keeps its
        // `(env_ptr, env_len)` signature on every target; consume the
        // bindings locally so the `unused_variables` lint passes
        // without an `#[allow]`.
        let _ = env_ptr;
        let _ = env_len;
    }
}

#[cfg(target_arch = "wasm32")]
fn run_body(env_ptr: i32, env_len: i32) {
    use super::boot;
    use super::mrb_slot::{MrbScope, MRB};
    use super::outcome_buffer::{write_outcome, write_panic};
    use crate::codec::Decoder;
    use crate::mruby::sys;
    use crate::outcome::{encode_outcome, Outcome, Panic};

    // See `eval_body` for the MRB scope-guard rationale — declared
    // early so every `return write_panic(...)` clears the slot.
    let _mrb_scope = MrbScope;

    let preamble = match boot::read_preamble() {
        Ok(p) => p,
        Err(panic) => return write_panic(panic),
    };
    let snippets = match boot::read_snippets() {
        Ok(s) => s,
        Err(panic) => return write_panic(panic),
    };

    let kobako = match boot::open_with_preamble(&preamble) {
        Ok(k) => k,
        Err(panic) => return write_panic(panic),
    };
    let mrb = MRB.as_ref().expect("MRB installed by open_with_preamble");

    // Baseline snapshot of top-level constants taken after kobako
    // install + preamble materialisation but before snippet replay.
    // Used to compute the E-27 `details:` payload — subtracting this
    // baseline from a post-replay snapshot yields exactly the
    // constants the preloaded snippets contributed
    // (docs/behavior.md B-31 / E-27).
    let baseline_constants = kobako.top_level_constants();

    if let Err(panic) = boot::replay_snippets(mrb, &kobako, &snippets) {
        return write_panic(panic);
    }

    // SAFETY: `(env_ptr, env_len)` were produced by the host's
    // `Instance::write_envelope`, which allocates the buffer via
    // `__kobako_alloc` (wasi-libc `malloc`) inside this same wasm
    // instance and then writes the envelope bytes verbatim. The buffer
    // lives for the duration of the `__kobako_run` call — wasi-libc's
    // allocator does not relocate live allocations. Reading
    // `env_len` bytes from `env_ptr` is therefore in-bounds for the
    // current instance's linear memory.
    let env_slice: &[u8] = if env_len == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(env_ptr as usize as *const u8, env_len as usize) }
    };

    let envelope = {
        let mut dec = Decoder::new(env_slice);
        match dec.read_value() {
            Ok(v) => v,
            Err(_) => {
                return write_panic(Panic {
                    origin: "sandbox".into(),
                    class: "Kobako::Transport::WireError".into(),
                    message: "failed to decode invocation envelope".into(),
                    backtrace: Vec::new(),
                    details: None,
                });
            }
        }
    };
    let invocation = match parse_invocation(envelope) {
        Ok(inv) => inv,
        Err(err) => {
            return write_panic(Panic {
                origin: "sandbox".into(),
                class: "Kobako::Transport::WireError".into(),
                message: err.message().into(),
                backtrace: Vec::new(),
                details: None,
            });
        }
    };

    // Resolve entrypoint Symbol against top-level `Object`. The whole
    // dispatch — const lookup (E-27), `respond_to?(:call)` gate
    // (E-28), and the `target.call(*args, **kwargs)` invocation —
    // runs through the mruby C API. No Ruby trampoline, no global
    // variable injection.
    let target_sym = mrb.intern_str(mrb.str_new(invocation.target.as_bytes()));
    // SAFETY: the cached `object_class` pointer was produced by the
    // same `mrb_state` and is GC-stable for the VM's lifetime.
    let object_value = unsafe { mrb.object_class().as_value(mrb) };

    if !object_value.const_defined(mrb, target_sym) {
        // Compute the snippet-contributed constants by subtracting the
        // pre-replay baseline from the current top-level set. Wrapped
        // as `{ "available" => [Sym, ...] }` so the host decoder can
        // pull the Array via `panic.details["available"]`.
        use std::collections::HashSet;
        let baseline_set: HashSet<&String> = baseline_constants.iter().collect();
        let post_constants = kobako.top_level_constants();
        let available: Vec<Value> = post_constants
            .into_iter()
            .filter(|name| !baseline_set.contains(name))
            .map(Value::Sym)
            .collect();
        let details = Value::Map(vec![(
            Value::Str("available".into()),
            Value::Array(available),
        )]);
        return write_panic(Panic {
            origin: "sandbox".into(),
            class: "Kobako::SandboxError".into(),
            message: format!("undefined entrypoint: {}", invocation.target),
            backtrace: Vec::new(),
            details: Some(details),
        });
    }

    let target_val = object_value.const_get(mrb, target_sym);
    if let Some(panic) = boot::take_pending_panic(mrb, &kobako) {
        // `mrb_const_get` only sets `mrb->exc` for genuinely undefined
        // constants; the `mrb_const_defined` gate above should make
        // this branch unreachable. If it ever fires (e.g. autoload
        // raised), surface the underlying panic verbatim so we never
        // silently swallow it.
        return write_panic(panic);
    }

    let call_sym = mrb.intern_cstr(c"call");
    if !target_val.respond_to(mrb, call_sym) {
        return write_panic(Panic {
            origin: "sandbox".into(),
            class: "Kobako::SandboxError".into(),
            message: format!("entrypoint {} does not respond to :call", invocation.target),
            backtrace: Vec::new(),
            details: None,
        });
    }

    // Build argv = [*args, kwargs?] where the trailing kwargs Hash is
    // appended as a positional argument (omitted when empty so a
    // `def call(*a)` entrypoint does not see an unwanted Hash tail).
    //
    // mruby C API limitation: `mrb_funcall_argv` and the entire
    // `mrb_funcall_*` family force `ci->nk = 0` on entry
    // (`vendor/mruby/src/vm.c:740` — "funcall does not support keyword
    // arguments"), so callers cannot mark the trailing Hash as a
    // kwargs splat. Entrypoints therefore see kwargs as the last
    // positional argument and must accept it as a plain `Hash` (e.g.
    // `def call(req, opts = {})` rather than `def call(req,
    // multiplier: 1)`). docs/behavior.md B-31 carries the
    // sandbox-visible side of this contract.
    let Value::Array(arg_items) = invocation.args else {
        // `parse_invocation` guarantees Array; the irrefutable shape
        // is reasserted here for the compiler.
        return write_panic(Panic {
            origin: "sandbox".into(),
            class: "Kobako::Transport::WireError".into(),
            message: "invocation envelope args must be an array".into(),
            backtrace: Vec::new(),
            details: None,
        });
    };
    let Value::Map(kwargs_pairs) = invocation.kwargs else {
        return write_panic(Panic {
            origin: "sandbox".into(),
            class: "Kobako::Transport::WireError".into(),
            message: "invocation envelope kwargs must be a map".into(),
            backtrace: Vec::new(),
            details: None,
        });
    };
    let kwargs_present = !kwargs_pairs.is_empty();
    let mut argv: Vec<sys::Value> = arg_items
        .into_iter()
        .map(|v| kobako.to_mrb_value(v))
        .collect();
    if kwargs_present {
        argv.push(kobako.to_mrb_value(Value::Map(kwargs_pairs)));
    }

    let result_val = target_val.call_argv(mrb, call_sym, &argv);

    if let Some(panic) = boot::take_pending_panic(mrb, &kobako) {
        write_panic(panic);
        return;
    }

    let codec_value = kobako.to_codec_outcome(result_val);
    match encode_outcome(&Outcome::Value(codec_value)) {
        Ok(bytes) => write_outcome(bytes),
        Err(_) => write_panic(Panic {
            origin: "sandbox".into(),
            class: "Kobako::Transport::WireError".into(),
            message: "result envelope encode failed".into(),
            backtrace: Vec::new(),
            details: None,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_invocation_accepts_complete_envelope() {
        let envelope = Value::Map(vec![
            (
                Value::Str("entrypoint".into()),
                Value::Sym("Greeter".into()),
            ),
            (
                Value::Str("args".into()),
                Value::Array(vec![Value::Int(42)]),
            ),
            (
                Value::Str("kwargs".into()),
                Value::Map(vec![(Value::Sym("flag".into()), Value::Bool(true))]),
            ),
        ]);
        let inv = parse_invocation(envelope).unwrap();
        assert_eq!(inv.target, "Greeter");
        assert_eq!(inv.args, Value::Array(vec![Value::Int(42)]));
        assert!(matches!(inv.kwargs, Value::Map(_)));
    }

    #[test]
    fn parse_invocation_defaults_missing_args_and_kwargs() {
        let envelope = Value::Map(vec![(
            Value::Str("entrypoint".into()),
            Value::Sym("Greeter".into()),
        )]);
        let inv = parse_invocation(envelope).unwrap();
        assert_eq!(inv.args, Value::Array(Vec::new()));
        assert_eq!(inv.kwargs, Value::Map(Vec::new()));
    }

    #[test]
    fn parse_invocation_rejects_non_map() {
        let envelope = Value::Array(Vec::new());
        assert_eq!(parse_invocation(envelope), Err(InvocationError::NotMap));
    }

    #[test]
    fn parse_invocation_rejects_missing_entrypoint() {
        let envelope = Value::Map(vec![(Value::Str("args".into()), Value::Array(Vec::new()))]);
        assert_eq!(
            parse_invocation(envelope),
            Err(InvocationError::MissingEntrypoint)
        );
    }

    #[test]
    fn parse_invocation_rejects_non_symbol_entrypoint() {
        let envelope = Value::Map(vec![(
            Value::Str("entrypoint".into()),
            Value::Str("Greeter".into()),
        )]);
        assert_eq!(
            parse_invocation(envelope),
            Err(InvocationError::MissingEntrypoint)
        );
    }

    #[test]
    fn parse_invocation_ignores_unknown_keys() {
        let envelope = Value::Map(vec![
            (
                Value::Str("entrypoint".into()),
                Value::Sym("Greeter".into()),
            ),
            (
                Value::Str("future_field".into()),
                Value::Str("ignored".into()),
            ),
        ]);
        let inv = parse_invocation(envelope).unwrap();
        assert_eq!(inv.target, "Greeter");
    }

    #[test]
    fn invocation_error_messages_match_panic_text() {
        assert_eq!(
            InvocationError::NotMap.message(),
            "invocation envelope must be a msgpack map"
        );
        assert_eq!(
            InvocationError::MissingEntrypoint.message(),
            "invocation envelope missing entrypoint Symbol"
        );
    }
}
