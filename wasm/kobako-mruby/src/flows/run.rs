//! `__kobako_run` — entrypoint dispatch entry.
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
//!    with the snippet's backtrace attribution
//!    and returns.
//! 2. Decode the invocation envelope from `(env_ptr, env_len)` via
//!    `parse_invocation`. Decode failure writes a Panic envelope.
//! 3. Resolve the entrypoint Symbol against top-level `Object` via
//!    `sys::mrb_const_defined` and confirm the constant
//!    responds to `:call` via `sys::mrb_respond_to`. Each
//!    failure writes a Panic envelope directly with the SPEC-mandated
//!    `Kobako::SandboxError` class string.
//! 4. Invoke `target.call(*args, **kwargs)` through `mrb_funcall_argv`
//!    by concatenating the decoded args Array and (when non-empty)
//!    appending the kwargs Hash as the trailing element — the same
//!    layout `Method#call` uses internally. Serialize the return
//!    value as a Result envelope or convert the pending mruby
//!    exception into a Panic envelope.

#[cfg(any(mruby_linked, test))]
use kobako_codec::codec::Value;

/// Decoded invocation envelope. `target` is the entrypoint constant
/// name (a Symbol on the codec side); the parser has already narrowed
/// `args` to the array's elements and `kwargs` to the map's pairs, so
/// callers hand them straight to
/// `crate::runtime::Kobako::to_mrb_value` without re-checking.
#[cfg(any(mruby_linked, test))]
#[derive(Debug, PartialEq)]
pub(super) struct Invocation {
    pub target: String,
    pub args: Vec<Value>,
    pub kwargs: Vec<(Value, Value)>,
}

/// Reasons the invocation envelope failed to decode. Each variant
/// carries the host-visible Panic message verbatim; the wrapper at
/// `__kobako_run` folds the variant back into a
/// `Kobako::Transport::Error` Panic.
#[cfg(any(mruby_linked, test))]
#[derive(Debug, PartialEq)]
pub(super) enum InvocationError {
    /// Envelope was not a msgpack map.
    NotMap,
    /// `entrypoint` key was absent or its value was not a Symbol.
    MissingEntrypoint,
    /// `args` key was present but its value was not an Array.
    ArgsNotArray,
    /// `kwargs` key was present but its value was not a Map.
    KwargsNotMap,
}

#[cfg(any(mruby_linked, test))]
impl InvocationError {
    pub(super) fn message(&self) -> &'static str {
        match self {
            Self::NotMap => "malformed invocation request",
            Self::MissingEntrypoint => "invocation request is missing an entrypoint",
            Self::ArgsNotArray => "invocation arguments must be an array",
            Self::KwargsNotMap => "invocation keyword arguments must be a map",
        }
    }
}

/// Parse a decoded msgpack `Value` into an `Invocation`. Unknown
/// keys are silently ignored for forward compatibility; `args` /
/// `kwargs` default to empty when absent but must carry their wire
/// shape when present. Pure parser — host-buildable for unit testing.
#[cfg(any(mruby_linked, test))]
pub(super) fn parse_invocation(envelope: Value) -> Result<Invocation, InvocationError> {
    let pairs = match envelope {
        Value::Map(p) => p,
        _ => return Err(InvocationError::NotMap),
    };
    let mut target: Option<String> = None;
    let mut args: Option<Vec<Value>> = None;
    let mut kwargs: Option<Vec<(Value, Value)>> = None;
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
            "args" => match v {
                Value::Array(items) => args = Some(items),
                _ => return Err(InvocationError::ArgsNotArray),
            },
            "kwargs" => match v {
                Value::Map(entries) => kwargs = Some(entries),
                _ => return Err(InvocationError::KwargsNotMap),
            },
            _ => {}
        }
    }
    let target = target.ok_or(InvocationError::MissingEntrypoint)?;
    Ok(Invocation {
        target,
        args: args.unwrap_or_default(),
        kwargs: kwargs.unwrap_or_default(),
    })
}

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
    use kobako_codec::codec::{Decoder, Encode};
    use kobako_codec::outcome::{Outcome, Panic};
    use kobako_core::abi::{write_outcome, write_panic};

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

    // Baseline snapshot of top-level constants taken after kobako
    // install + preamble materialisation but before snippet replay.
    // Used to compute the `details:` payload — subtracting this
    // baseline from a post-replay snapshot yields exactly the
    // constants the preloaded snippets contributed.
    let baseline_constants = kobako.top_level_constants();

    if let Err(panic) = boot::replay_snippets(&kobako, &snippets) {
        return write_panic(panic);
    }

    let envelope = {
        let mut dec = Decoder::new(env);
        match dec.read_only_value() {
            Ok(v) => v,
            Err(_) => {
                return write_panic(boot::transport_panic(
                    "failed to decode the invocation request",
                ));
            }
        }
    };
    let invocation = match parse_invocation(envelope) {
        Ok(inv) => inv,
        Err(err) => {
            return write_panic(boot::transport_panic(err.message()));
        }
    };

    // Resolve entrypoint Symbol against top-level `Object`. The whole
    // dispatch — const lookup, `respond_to?(:call)` gate,
    // and the `target.call(*args, **kwargs)` invocation —
    // runs through the mruby C API. No Ruby trampoline, no global
    // variable injection.
    let target_sym = mrb.intern_str(mrb.str_new(invocation.target.as_bytes()).as_value());
    // SAFETY: the cached `object_class` pointer was produced by the
    // same `mrb_state` and is GC-stable for the VM's lifetime.
    let object_value = unsafe { mrb.object_class().to_value(mrb) };

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
    // `mrb_funcall_*` family force `ci->nk = 0` on entry ("funcall does
    // not support keyword arguments"), so callers cannot mark the
    // trailing Hash as a kwargs splat. Entrypoints therefore see kwargs
    // as the last
    // positional argument and must accept it as a plain `Hash` (e.g.
    // `def call(req, opts = {})` rather than `def call(req,
    // multiplier: 1)`).
    // An argument the guest cannot represent — an integer outside the
    // 32-bit range — fails the invocation rather than reaching the
    // entrypoint with a saturated value (docs/wire-codec.md § Integer
    // Range).
    let mut argv: Vec<beni::Value> = match invocation
        .args
        .into_iter()
        .map(|v| kobako.to_mrb_value(v))
        .collect()
    {
        Ok(argv) => argv,
        Err(err) => return write_panic(boot::transport_panic(err.message())),
    };
    if !invocation.kwargs.is_empty() {
        match kobako.to_mrb_value(Value::Map(invocation.kwargs)) {
            Ok(kwargs_val) => argv.push(kwargs_val),
            Err(err) => return write_panic(boot::transport_panic(err.message())),
        }
    }

    let result_val = match target_val.funcall_argv(mrb, call_sym, &argv) {
        Ok(v) => v,
        Err(err) => return write_panic(boot::panic_from_error(&kobako, err)),
    };

    let Some(codec_value) = kobako.try_codec_value(result_val) else {
        return write_panic(boot::unrepresentable_return_panic(&kobako, result_val));
    };
    match Outcome::Value(codec_value).encode() {
        Ok(bytes) => write_outcome(bytes),
        Err(_) => write_panic(boot::transport_panic("result envelope encode failed")),
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
        assert_eq!(inv.args, vec![Value::Int(42)]);
        assert_eq!(
            inv.kwargs,
            vec![(Value::Sym("flag".into()), Value::Bool(true))]
        );
    }

    #[test]
    fn parse_invocation_defaults_missing_args_and_kwargs() {
        let envelope = Value::Map(vec![(
            Value::Str("entrypoint".into()),
            Value::Sym("Greeter".into()),
        )]);
        let inv = parse_invocation(envelope).unwrap();
        assert!(inv.args.is_empty());
        assert!(inv.kwargs.is_empty());
    }

    #[test]
    fn parse_invocation_rejects_non_array_args() {
        let envelope = Value::Map(vec![
            (
                Value::Str("entrypoint".into()),
                Value::Sym("Greeter".into()),
            ),
            (Value::Str("args".into()), Value::Int(1)),
        ]);
        assert_eq!(
            parse_invocation(envelope),
            Err(InvocationError::ArgsNotArray)
        );
    }

    #[test]
    fn parse_invocation_rejects_non_map_kwargs() {
        let envelope = Value::Map(vec![
            (
                Value::Str("entrypoint".into()),
                Value::Sym("Greeter".into()),
            ),
            (Value::Str("kwargs".into()), Value::Array(Vec::new())),
        ]);
        assert_eq!(
            parse_invocation(envelope),
            Err(InvocationError::KwargsNotMap)
        );
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
            "malformed invocation request"
        );
        assert_eq!(
            InvocationError::MissingEntrypoint.message(),
            "invocation request is missing an entrypoint"
        );
        assert_eq!(
            InvocationError::ArgsNotArray.message(),
            "invocation arguments must be an array"
        );
        assert_eq!(
            InvocationError::KwargsNotMap.message(),
            "invocation keyword arguments must be a map"
        );
    }
}
