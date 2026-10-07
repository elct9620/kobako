//! Method bodies the Kobako surface registers with mruby, and the helpers
//! they share. Each `beni::method!` body reports failure as `Err` for the
//! macro to raise, so nothing long-jumps over a Rust frame and
//! `resolve_raw` is the one `unsafe` left here.
//!
//! The proxy reaches the host through `crate::dispatch`, the same seam a
//! capability gem uses, so the built-in proxy holds no privilege over one.

use beni::prelude::*;
use beni::{Mrb, Proc, RArray, RHash, Symbol, Value};

use crate::codec::CodecError;
use crate::runtime::codec_slot;

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

/// `NoMethodError` for a reflection method the guest proxy refuses to
/// forward, naming the method without leaking host detail.
fn reflection_blocked(mrb: &Mrb, method_name: &str) -> beni::Error {
    match mrb.exc_get(c"NoMethodError") {
        Ok(nomethod) => beni::Error::new(
            mrb,
            nomethod,
            &format!("{method_name} is not a Kobako Service method"),
        ),
        Err(err) => err,
    }
}

/// The payload codec is this side's to run: the transport beneath routes
/// the Call without reading a byte of it.
///
/// The helper reads the call frame itself, so a caller must not have
/// consumed the arglist before reaching it.
fn forward_to_dispatch(
    kobako: super::Kobako,
    target: kobako_transport::envelope::Target<'_>,
    sym_err_msg: &str,
    envelope_err_msg: &str,
) -> Result<Value, beni::Error> {
    use crate::refusal::Position;

    use crate::call::{dispatch, DispatchError};
    use kobako_transport::envelope::FaultKind;

    let args =
        beni::scan_args::scan_args::<(Symbol,), (), RArray, (), RHash, Option<Proc>>(kobako.mrb())?;
    let (method_sym,) = args.required;
    let rest: Vec<Value> = args.splat.entries(kobako.mrb()).collect();
    let kwargs_hash = args.keywords;
    let block = args.block;

    let method_name = match kobako.mrb().sym_name(method_sym.into()) {
        Some(name) => name,
        None => return Err(kobako.transport_error(sym_err_msg)),
    };

    // Guest-side mirror of the host's reflection rejection:
    // refuse to forward an ambient reflection / eval name. Non-authoritative
    // — the host re-checks on the resolved method owner.
    if REFLECTION_DENYLIST.contains(&method_name.as_str()) {
        return Err(reflection_blocked(kobako.mrb(), &method_name));
    }

    // An argument (or kwargs value) with no representation in this guest's
    // schema is rejected at the dispatch call site rather than coerced to
    // an Object#to_s string, uniform with the return / yield rejection.
    let payload = match codec_slot::get().encode_call_arguments(&kobako, &rest, kwargs_hash) {
        Ok(payload) => payload,
        Err(err) => return Err(refusal(&kobako, Position::CallArguments, err)),
    };

    // The block parks for the call's duration inside `dispatch`, so every
    // raise above this line — an unreadable symbol, a denied name, an
    // argument this schema cannot carry — long-jumps past no guard.
    let answer = dispatch(target, &method_name, block, &payload);
    // Asked on every arm, and only for this call's own block: a failure
    // the Service rescued is spent, and one still held for another block
    // belongs to the dispatch that parked it.
    let raised =
        block.and_then(|one| super::raised_block::RAISED_BLOCK.take_for(kobako.mrb(), one));
    match answer {
        // A dispatch return value the guest cannot represent raises in the
        // calling guest code (docs/wire/payload-msgpack.md § Integer Range).
        Ok(body) => match codec_slot::get().decode_reply_value(&kobako, &body) {
            Ok(value) => Ok(value),
            Err(err) => Err(refusal(&kobako, Position::ReplyValue, err)),
        },
        // The fault arm is the normal path for a Service raising. The
        // envelope typed it, so there is nothing left to decode and no
        // codec to consult.
        Err(DispatchError::Fault(fault)) => match (fault.kind, raised) {
            // The Service did not rescue what this frame's own block
            // raised, so the exception continues from where it was
            // raised — the same thing it would do with no host in
            // between. A `block` fault with nothing held is a peer
            // reporting a failure this frame did not produce, which has
            // no exception to continue and takes the ordinary path.
            // The exception continues as itself rather than as a
            // reconstruction; `exc` was held live by the GC root the take
            // just released.
            (FaultKind::Block, Some(exc)) => Err(beni::Error::Exception(exc)),
            _ => Err(kobako.service_error(&fault)),
        },
        // Anything that is not the Service's own fault means the exchange
        // did not complete, which reaches the guest as a wire fault.
        Err(_) => Err(kobako.transport_error(envelope_err_msg)),
    }
}

/// The attribution itself is `crate::refusal`'s; this only delivers it
/// into a running script.
///
/// `Kobako::Transport::Error` is kobako's own namespaced constant, which
/// no name lookup reaches, so it comes from the class `Kobako` already
/// holds; every other class the table names is an mruby core class and is
/// fetched by name. A lookup that fails there means the interpreter is
/// missing a core class, which is not a condition to raise something else
/// about.
fn refusal(
    kobako: &super::Kobako,
    position: crate::refusal::Position,
    err: CodecError,
) -> beni::Error {
    let refusal = crate::refusal::at(position, err);
    if refusal.class == crate::refusal::TRANSPORT_ERROR {
        return kobako.transport_error(&refusal.message);
    }
    let name = std::ffi::CString::new(refusal.class).expect("a class name carries no interior NUL");
    match kobako.mrb().exc_get(&*name) {
        Ok(class) => beni::Error::new(kobako.mrb(), class, &refusal.message),
        Err(err) => err,
    }
}

/// The Call `Target` follows the receiver's identity: an exact
/// `Kobako::Handle` yields its id, a class its constant name. Any other
/// receiver — a subclass of `Kobako::Handle`, or a foreign object that
/// mixed in the module — has no target and is refused in-guest, so a guest
/// cannot drive a Handle-targeted dispatch off arbitrary instance state by
/// fabricating a proxy holder.
pub(crate) fn proxy_method_missing(
    mrb: &Mrb,
    self_: Value,
    _args: &[Value],
) -> Result<Value, beni::Error> {
    use kobako_transport::envelope::Target;

    // SAFETY: `mrb` is live for this bridge frame and install has run
    // (the module was registered by it).
    let kobako = unsafe { super::Kobako::resolve_raw(mrb) };

    // A path target borrows the name for the Call it rides in, so the
    // name outlives the target rather than the branch that read it.
    let class_name;
    let target = if self_.is_instance_of(mrb, kobako.registrations.handle_class) {
        // An exact `Kobako::Handle` instance carrying its id ivar. Exact,
        // not `is_kind_of`: the decoder mints only `Kobako::Handle`, so a
        // guest subclass of it is a fabrication and derives no target.
        Target::Handle(kobako.extract_handle_id(self_))
    } else if let Some(class) = beni::RClass::from_value(self_) {
        // A bound-Service constant, reached through `Kobako::Proxy`
        // extended onto its singleton class.
        class_name = class.name(kobako.mrb());
        Target::Path(&class_name)
    } else {
        return Err(no_target(mrb, self_));
    };

    forward_to_dispatch(
        kobako,
        target,
        "proxy method symbol name is null",
        "transport envelope error (proxy dispatch)",
    )
}

fn no_target(mrb: &Mrb, self_: Value) -> beni::Error {
    match mrb.exc_get(c"NoMethodError") {
        Ok(nomethod) => beni::Error::new(
            mrb,
            nomethod,
            &format!("{} is not a Kobako dispatch target", self_.classname(mrb)),
        ),
        Err(err) => err,
    }
}

/// `Kobako::Handle.new` / `.allocate` both raise, so an exact
/// `Kobako::Handle` arises only from the wire decoder's `mrb_obj_new`; with
/// guest construction closed, a `Kobako::Handle` receiver in
/// `proxy_method_missing` is always host-issued.
pub(crate) fn handle_not_constructible(
    mrb: &Mrb,
    _self: Value,
    _args: &[Value],
) -> Result<Value, beni::Error> {
    Err(match mrb.exc_get(c"NoMethodError") {
        Ok(nomethod) => beni::Error::new(
            mrb,
            nomethod,
            "Kobako::Handle is a host-issued capability reference, not a constructible class",
        ),
        Err(err) => err,
    })
}

pub(crate) fn handle_initialize(mrb: &Mrb, self_: Value, id: Value) -> Result<(), beni::Error> {
    // SAFETY: `mrb` is live for this bridge frame and install has run.
    let kobako = unsafe { super::Kobako::resolve_raw(mrb) };
    kobako.set_handle_id(self_, id)
}

/// `Kobako::Handle#initialize_copy(orig)` C bridge. mruby copies the id ivar
/// into the fresh copy before invoking this hook, so it only freezes the
/// copy — making a `dup` (which otherwise yields an unfrozen copy) immutable
/// like the decoder-minted original, so the guest cannot mint a re-pointable
/// Handle by duplicating one. A `clone` already inherits the frozen flag.
pub(crate) fn handle_initialize_copy(mrb: &Mrb, self_: Value, _orig: Value) -> Value {
    self_.freeze(mrb)
}

/// Always `true`: every call dispatches through `method_missing` to the
/// host, so probing via `respond_to?` must succeed.
pub(crate) fn proxy_respond_to_missing(_mrb: &Mrb, _self_: Value, _args: &[Value]) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::REFLECTION_DENYLIST;

    // The escape vectors that motivated the reflection denylist must stay
    // refused guest-side:
    // the `send` family pivots into the private `Kernel#eval` / `#system`
    // surface, the `eval` family runs guest-authored strings, and the gadget
    // reflectors (`binding` reaches `Binding#eval`) hand back host internals.
    // @behavior T-182
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
    // @behavior T-183
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
