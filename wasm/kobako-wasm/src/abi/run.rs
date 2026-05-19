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
//! 3. Stash `args` / `kwargs` / `entrypoint` in mruby globals and
//!    evaluate the dispatch wrapper under filename `(dispatch)`. The
//!    wrapper checks `Object.const_defined?` (E-27) and
//!    `respond_to?(:call)` (E-28) before invoking
//!    `target.call(*args, **kwargs)`.
//! 4. Serialize the return value as a Result envelope or convert the
//!    pending mruby exception into a Panic envelope.

#[cfg(any(target_arch = "wasm32", test))]
use crate::codec::Value;

/// Decoded invocation envelope. `target` is the entrypoint constant
/// name (the wire-level Symbol); `args` is always a [`Value::Array`]
/// and `kwargs` always a [`Value::Map`] — callers can hand them
/// straight to [`crate::kobako::Kobako::wire_value_to_mrb`] without
/// re-checking.
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
/// `Kobako::RPC::WireError` Panic.
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
        let _ = env_ptr;
        let _ = env_len;
    }
}

#[cfg(target_arch = "wasm32")]
fn run_body(env_ptr: i32, env_len: i32) {
    use super::boot;
    use super::outcome_buffer::{write_outcome, write_panic};
    use crate::codec::Decoder;
    use crate::cstr;
    use crate::mruby::sys;
    use crate::outcome::{encode_outcome, Outcome, Panic};

    let preamble = match boot::read_preamble() {
        Ok(p) => p,
        Err(panic) => return write_panic(panic),
    };
    let snippets = match boot::read_snippets() {
        Ok(s) => s,
        Err(panic) => return write_panic(panic),
    };

    let (mrb, kobako) = match boot::open_with_preamble(&preamble) {
        Ok(pair) => pair,
        Err(panic) => return write_panic(panic),
    };

    if let Err(panic) = boot::replay_snippets(&mrb, &kobako, &snippets) {
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
                    class: "Kobako::RPC::WireError".into(),
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
                class: "Kobako::RPC::WireError".into(),
                message: err.message().into(),
                backtrace: Vec::new(),
                details: None,
            });
        }
    };

    let target_mrb = kobako.wire_value_to_mrb(Value::Sym(invocation.target.clone()));
    let args_mrb = kobako.wire_value_to_mrb(invocation.args);
    let kwargs_mrb = kobako.wire_value_to_mrb(invocation.kwargs);
    // SAFETY: bridge frame — all `mrb_value`s come from the same VM,
    // and `mrb_intern_cstr` accepts the static NUL-terminated names.
    unsafe {
        sys::mrb_gv_set(
            mrb.as_ptr(),
            sys::mrb_intern_cstr(mrb.as_ptr(), cstr!("$__kobako_run_target__")),
            target_mrb,
        );
        sys::mrb_gv_set(
            mrb.as_ptr(),
            sys::mrb_intern_cstr(mrb.as_ptr(), cstr!("$__kobako_run_args__")),
            args_mrb,
        );
        sys::mrb_gv_set(
            mrb.as_ptr(),
            sys::mrb_intern_cstr(mrb.as_ptr(), cstr!("$__kobako_run_kwargs__")),
            kwargs_mrb,
        );
    }

    // Wrapper checks `Object.const_defined?` (E-27) and
    // `respond_to?(:call)` (E-28) before invoking
    // `target.call(*args, **kwargs)`. Loaded under filename
    // `(dispatch)` so a wrapper-level failure carries a clear locator;
    // entrypoint failures keep the `(snippet:Name)` frame from B-32 in
    // their backtrace.
    const DISPATCH_SRC: &str = r#"
__t = $__kobako_run_target__
if Object.const_defined?(__t)
  __c = Object.const_get(__t)
  if __c.respond_to?(:call)
    __c.call(*$__kobako_run_args__, **$__kobako_run_kwargs__)
  else
    raise Kobako::RPC::WireError.new("entrypoint #{__t} does not respond to :call")
  end
else
  raise Kobako::RPC::WireError.new("undefined entrypoint: #{__t}")
end
"#;
    let cxt = unsafe { sys::mrb_ccontext_new(mrb.as_ptr()) };
    if cxt.is_null() {
        return write_panic(boot::boot_panic("mrb_ccontext_new returned NULL"));
    }
    unsafe { sys::mrb_ccontext_filename(mrb.as_ptr(), cxt, cstr!("(dispatch)")) };
    let result_val = unsafe {
        sys::mrb_load_nstring_cxt(
            mrb.as_ptr(),
            DISPATCH_SRC.as_ptr() as *const core::ffi::c_char,
            DISPATCH_SRC.len(),
            cxt,
        )
    };
    unsafe { sys::mrb_ccontext_free(mrb.as_ptr(), cxt) };

    if let Some(panic) = boot::take_pending_panic(&mrb, &kobako) {
        write_panic(panic);
        return;
    }

    let wire_value = kobako.mrb_value_to_wire_outcome(result_val);
    match encode_outcome(&Outcome::Value(wire_value)) {
        Ok(bytes) => write_outcome(bytes),
        Err(_) => write_panic(Panic {
            origin: "sandbox".into(),
            class: "Kobako::RPC::WireError".into(),
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
