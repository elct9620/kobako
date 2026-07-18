//! Method bodies registered with mruby at install time.
//!
//! Every function here is a typed `beni::method!` body
//! (`fn(&Mrb, Value, …) -> Value`); the macro generates the raw
//! `mrb_func_t` bridge mruby invokes. The registrations happen in
//! `super::Kobako::init`; the bodies re-enter the boundary by
//! resolving a `Kobako` token via `super::Kobako::resolve_raw` and
//! then call safe methods.
//!
//! ## Dispatch chain
//!
//! ```text
//!   user_script:    MyService::KV.get(:user_42)
//!        │
//!        │ (no method named `get`; the call falls through to the
//!        │  `method_missing` the `Kobako::Proxy` module contributes —
//!        │  at class level for a bound constant that extended the module,
//!        │  at instance level for a Handle that included it)
//!        ▼
//!   proxy_method_missing(mrb, self=KV.class)
//!        │
//!        │ (derive Target from the receiver's identity: a Kobako::Handle
//!        │  instance → Target::Handle from its `@__kobako_id__` ivar, a
//!        │  class → Target::Path from its name; any other receiver has no
//!        │  target and is refused in-guest)
//!        ▼
//!   forward_to_dispatch(Target::Path(target_str), ...)
//!        ▼
//!   kobako_core::transport::proxy::invoke(...)
//! ```
//!
//! `proxy_method_missing` is the single forwarding entry the
//! `Kobako::Proxy` module contributes to both proxy shapes. It derives the
//! `Target` from the receiver's positive identity — a `Kobako::Handle`
//! instance by its id, a class by its constant path — and refuses in-guest
//! any receiver that is neither, so a fabricated `Kobako::Proxy` holder
//! cannot drive a dispatch off arbitrary instance state. The BlockFrame
//! push, method-symbol extraction, args/kwargs unpacking, host round-trip,
//! and result conversion all live in `forward_to_dispatch`.
//!
//! ## Safety
//!
//! The `method!`-generated bridges hand each body a borrowed `&Mrb`,
//! so the bodies are safe Rust with explicit `unsafe { ... }` blocks
//! only at the remaining FFI call sites (`resolve_raw`, the divergent
//! raises). A divergent raise long-jumps over the macro's bridge
//! frame, which holds no values needing `Drop` — the same contract
//! the raw bridges upheld.

use beni::{Module, Mrb, Value};

/// Ambient reflection / eval method names the guest proxy refuses to
/// forward. This is a best-effort opacity mirror,
/// not a security boundary: the host's owner-based guard re-checks every
/// dispatch and stays the complete authority, so this hand-maintained name
/// list may lag it (a name only the host rejects is still caught) without
/// weakening the sandbox. The callable allowlist (`call` / `[]` / `yield` /
/// `arity` / `lambda?`) is absent so a bound lambda stays invocable.
const REFLECTION_DENYLIST: &[&str] = &[
    "send",
    "__send__",
    "public_send",
    "eval",
    "instance_eval",
    "instance_exec",
    "class_eval",
    "module_eval",
    "binding",
    "method",
    "public_method",
    "instance_method",
    "define_method",
    "define_singleton_method",
    "const_get",
    "const_set",
    "instance_variable_get",
    "instance_variable_set",
    "singleton_class",
    "curry",
    "to_proc",
    "receiver",
    "unbind",
];

/// Raise `NoMethodError` for a reflection method the guest proxy refuses
/// to forward, naming the method without leaking host detail.
fn raise_reflection_blocked(mrb: &Mrb, method_name: &str) -> Value {
    let nomethod = mrb
        .exc_get(c"NoMethodError")
        .expect("NoMethodError is an mruby core class");
    let message = std::ffi::CString::new(format!("{method_name} is not a Kobako Service method"))
        .unwrap_or_default();
    // SAFETY: bridge frame — mruby unwinds through `mrb_raise`, the same
    // exit path the Service / transport raises in the dispatch body take.
    unsafe { nomethod.raise(mrb, &message) }
}

/// Full guest→host dispatch from the active mruby call frame — the
/// shared body behind `proxy_method_missing`. The caller supplies the
/// `Target` it derived from its `self_` receiver (a class name for a
/// bound constant, a Handle id for a `Kobako::Handle` instance) plus two
/// error labels: `sym_err_msg` for a null method symbol, `envelope_err_msg`
/// for a transport envelope fault. Extracts the method symbol, args/kwargs,
/// and block; rounds the request through the host via
/// `kobako_core::transport::proxy::invoke`; and converts the result back
/// to an mruby value — raising `Kobako::ServiceError` on a Response.err and
/// `Kobako::Transport::Error` on an envelope fault (both raise paths
/// diverge). The `Kobako` token supplies only the VM-level primitives
/// (arg/result conversion, error raising); the dispatch orchestration
/// lives here, not on the token.
///
/// The helper runs `kobako.mrb().get_args::<NRestKwBlock>()` itself, so
/// callers must not have already consumed the arglist.
fn forward_to_dispatch(
    kobako: super::Kobako,
    target: kobako_codec::transport::Target,
    sym_err_msg: &core::ffi::CStr,
    envelope_err_msg: &core::ffi::CStr,
) -> Value {
    use super::block_stack::BlockFrame;
    use kobako_core::transport::proxy::{invoke, InvokeError};

    let (method_sym, rest, kwargs_hash, block) =
        kobako.mrb().get_args::<beni::format::NRestKwBlock>();

    // Push the block onto BLOCK_STACK for the duration of this bridge
    // frame; drops + pops automatically on return / mruby raise. The
    // wire-level `block_given` bit is the observable shadow of
    // the same fact.
    let block_given = !block.is_nil();
    let _block_frame = BlockFrame::push_if_block(block);

    let method_name = match kobako.mrb().sym_name(method_sym) {
        Some(name) => name,
        None => unsafe { kobako.raise_transport_error(sym_err_msg) },
    };

    // Guest-side mirror of the host's reflection rejection:
    // refuse to forward an ambient reflection / eval name. Non-authoritative
    // — the host re-checks on the resolved method owner.
    if REFLECTION_DENYLIST.contains(&method_name.as_str()) {
        return raise_reflection_blocked(kobako.mrb(), &method_name);
    }

    // An argument (or kwargs value) with no wire representation is rejected
    // at the guest dispatch call site rather than coerced to an Object#to_s
    // string, uniform with the return / yield rejection.
    let (args, kwargs) = match kobako.unpack_args_kwargs(rest, kwargs_hash) {
        Ok(unpacked) => unpacked,
        // SAFETY: bridge frame — mruby unwinds through `mrb_raise`.
        Err(unrep) => {
            let msg = std::ffi::CString::new(unrep.message()).unwrap_or_default();
            unsafe { kobako.raise_transport_error(&msg) }
        }
    };

    match invoke(target, &method_name, &args, &kwargs, block_given) {
        Ok(value) => match kobako.to_mrb_value(value) {
            Ok(mrb_value) => mrb_value,
            // A dispatch return value the guest cannot represent raises in
            // the calling guest code (docs/wire-codec.md § Integer Range).
            // SAFETY: bridge frame — mruby unwinds through `mrb_raise`.
            Err(err) => {
                let msg = std::ffi::CString::new(err.message()).unwrap_or_default();
                unsafe { kobako.raise_transport_error(&msg) }
            }
        },
        // SAFETY: bridge frame — mruby unwinds through `mrb_raise`.
        Err(InvokeError::Service(ex)) => unsafe { kobako.raise_service_error(&ex) },
        // SAFETY: as above.
        Err(_) => unsafe { kobako.raise_transport_error(envelope_err_msg) },
    }
}

/// `Kobako::Proxy#method_missing(name, *args)` C bridge — the single
/// forwarding entry the module contributes to both proxy shapes.
/// `Kobako::Proxy` is extended onto each bound-Service constant and included
/// into `Kobako::Handle`. The Request `Target` follows the receiver's
/// identity: an exact `Kobako::Handle` instance yields `Target::Handle`
/// from its `@__kobako_id__` ivar, and a class receiver yields
/// `Target::Path` from its constant name. Any other receiver — a subclass
/// of `Kobako::Handle`, or a foreign object that mixed in the module — has
/// no target and is refused in-guest (`raise_no_target`), so a guest cannot
/// drive a Handle-targeted dispatch off arbitrary instance state by
/// fabricating a proxy holder.
///
/// Forwards to `forward_to_dispatch`.
pub(crate) fn proxy_method_missing(mrb: &Mrb, self_: Value) -> Value {
    use kobako_codec::transport::Target;

    // SAFETY: `mrb` is live for this bridge frame and install has run
    // (the module was registered by it).
    let kobako = unsafe { super::Kobako::resolve_raw(mrb) };

    let target = if self_.is_instance_of(mrb, kobako.handle_class) {
        // An exact `Kobako::Handle` instance carrying its id ivar. Exact,
        // not `is_kind_of`: the decoder mints only `Kobako::Handle`, so a
        // guest subclass of it is a fabrication and derives no target.
        Target::Handle(kobako.extract_handle_id(self_))
    } else if self_.is_class() {
        // SAFETY: `is_class()` proves `self_` is class-tagged, so
        // `as_class_ptr` is valid — a bound-Service constant reached
        // through `Kobako::Proxy` extended onto its singleton class.
        let class = beni::RClass::from_raw(unsafe { self_.as_class_ptr() });
        Target::Path(class.name(kobako.mrb()))
    } else {
        return raise_no_target(mrb, self_);
    };

    forward_to_dispatch(
        kobako,
        target,
        c"proxy method symbol name is null",
        c"transport envelope error (proxy dispatch)",
    )
}

/// Refuse a dispatch from a receiver that mixed in `Kobako::Proxy` yet is
/// neither a `Kobako::Handle` nor a class: it carries no dispatch target,
/// so the call raises `NoMethodError` in-guest and sends no Request rather
/// than forwarding a target read off arbitrary instance state.
fn raise_no_target(mrb: &Mrb, self_: Value) -> Value {
    let nomethod = mrb
        .exc_get(c"NoMethodError")
        .expect("NoMethodError is an mruby core class");
    let message = std::ffi::CString::new(format!(
        "{} is not a Kobako dispatch target",
        self_.classname(mrb)
    ))
    .unwrap_or_default();
    // SAFETY: bridge frame — mruby unwinds through `mrb_raise`.
    unsafe { nomethod.raise(mrb, &message) }
}

/// `Kobako::Handle.new` / `.allocate` C bridge — singleton-class level.
/// Both raise `NoMethodError` so an exact `Kobako::Handle` arises only from
/// the wire decoder's `mrb_obj_new` (which bypasses these Ruby entries);
/// with guest construction closed, a `Kobako::Handle` receiver in
/// `proxy_method_missing` is always host-issued. `mrb_args_any()` makes the
/// raise fire regardless of arguments.
pub(crate) fn handle_not_constructible(mrb: &Mrb, _self: Value) -> Value {
    let nomethod = mrb
        .exc_get(c"NoMethodError")
        .expect("NoMethodError is an mruby core class");
    // SAFETY: bridge frame — mruby unwinds through `mrb_raise`.
    unsafe {
        nomethod.raise(
            mrb,
            c"Kobako::Handle is a host-issued capability reference, not a constructible class",
        )
    }
}

/// `Kobako::Handle#initialize(id)` C bridge. Stores the Handle integer
/// id into the `@__kobako_id__` instance variable via
/// `super::Kobako::set_handle_id`.
pub(crate) fn handle_initialize(mrb: &Mrb, self_: Value) -> Result<Value, beni::Error> {
    // SAFETY: `mrb` is live for this bridge frame and install has run.
    let kobako = unsafe { super::Kobako::resolve_raw(mrb) };
    let id_val = mrb.get_args::<beni::format::O>();
    kobako.set_handle_id(self_, id_val)?;
    Ok(Value::zeroed())
}

/// `respond_to_missing?(name, include_private)` C bridge, contributed by
/// the `Kobako::Proxy` module. Always returns `true` — every method call
/// is dispatched through `method_missing` to the host, so probing via
/// `respond_to?` must succeed. Class-level on a bound constant that
/// extended the module, instance-level on a `Kobako::Handle` that included
/// it.
pub(crate) fn proxy_respond_to_missing(_mrb: &Mrb, _self_: Value) -> Value {
    // No VM access needed: `Value::true_()` reads the sys-side immediates
    // cache, populated at install before any probe runs, so the raw
    // `mrb` pointer goes unused.
    Value::true_()
}

#[cfg(test)]
mod tests {
    use super::REFLECTION_DENYLIST;

    // The escape vectors that motivated the reflection denylist must stay
    // refused guest-side:
    // the `send` family pivots into the private `Kernel#eval` / `#system`
    // surface, the `eval` family runs guest-authored strings, and the gadget
    // reflectors (`binding` reaches `Binding#eval`) hand back host internals.
    #[test]
    fn denylist_covers_the_reflection_escape_vectors() {
        for name in [
            "send",
            "__send__",
            "public_send",
            "eval",
            "instance_eval",
            "instance_exec",
            "class_eval",
            "module_eval",
            "binding",
            "method",
            "public_method",
            "instance_method",
            "define_method",
            "define_singleton_method",
            "instance_variable_get",
            "instance_variable_set",
        ] {
            assert!(
                REFLECTION_DENYLIST.contains(&name),
                "{name} is a reflection escape vector and must stay on the guest denylist"
            );
        }
    }

    // The callable allowlist is expressed by absence from the denylist: a
    // bound lambda / Method stays invocable. Denying any of these would make
    // Service callables unreachable end to end.
    #[test]
    fn denylist_keeps_the_callable_allowlist_forwardable() {
        for name in ["call", "[]", "yield", "arity", "lambda?"] {
            assert!(
                !REFLECTION_DENYLIST.contains(&name),
                "{name} is the callable allowlist and must stay forwardable, not denied"
            );
        }
    }
}
