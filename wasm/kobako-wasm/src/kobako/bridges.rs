//! C-callable shims registered with mruby at install time.
//!
//! Every function here matches the `crate::mruby::sys::mrb_func_t`
//! signature so mruby can invoke it as a method body. The registrations
//! happen in `super::Kobako::install`; the bridges themselves
//! re-enter the boundary by resolving a `Kobako` token via
//! `super::Kobako::resolve_raw` and then call safe methods.
//!
//! ## Dispatch chain
//!
//! ```text
//!   user_script:    MyService::KV.get(:user_42)
//!        │
//!        │ (no instance method named `get`; class-level dispatch falls
//!        │  through to the singleton-class `method_missing` inherited
//!        │  from `Kobako::Member.singleton_class`)
//!        ▼
//!   member_method_missing(mrb, self=KV.class)
//!        │
//!        │ (extract method symbol + args; build kwargs hash from
//!        │  trailing Hash if present; resolve target string via
//!        │  `mrb_class_name(mrb, mrb_class_ptr(self))`)
//!        ▼
//!   forward_to_dispatch(Target::Path(target_str), ...)
//!        ▼
//!   crate::transport::proxy::invoke(...)
//! ```
//!
//! The two `method_missing` bridges live on the two `Kobako::Transport::Proxy`
//! subclasses: `member_method_missing` is the singleton-class shim on
//! `Kobako::Member` (Member *classes*, `Target::Path`) and
//! `handle_method_missing` is the instance shim on `Kobako::Handle`
//! (Handle *instances*, `Target::Handle`, docs/behavior.md B-17). The
//! two differ only in how they derive the `Target` from `self_`; the
//! BlockFrame push, method-symbol extraction, args/kwargs unpacking,
//! host round-trip, and result conversion all live in
//! `forward_to_dispatch`.
//!
//! ## Safety
//!
//! Each bridge is `unsafe extern "C" fn` because mruby invokes it from
//! the C side with a raw `*mut mrb_state` and a `Value` receiver.
//! Bodies open with `unsafe { Kobako::resolve_raw(mrb) }` to obtain the
//! safe `Kobako` token; from then on the work is safe Rust with
//! explicit `unsafe { ... }` blocks at each remaining FFI call site.

use crate::mruby::sys;
use crate::mruby::Value;

/// Full guest→host dispatch from the active mruby call frame — the
/// shared body of the two `method_missing` bridges. The caller supplies
/// the `Target` it derived from its `self_` receiver (a class name for
/// the `Kobako::Member` singleton-class shim, a Handle id for the
/// `Kobako::Handle` instance shim) plus two error labels: `sym_err_msg`
/// for a null method symbol, `envelope_err_msg` for a transport envelope
/// fault. Extracts the method symbol, args/kwargs, and block; rounds the
/// request through the host via `crate::transport::proxy::invoke`; and
/// converts the result back to an mruby value — raising
/// `Kobako::ServiceError` on a Response.err and
/// `Kobako::Transport::Error` on an envelope fault (both raise paths
/// diverge). The `Kobako` token supplies only the VM-level primitives
/// (arg/result conversion, error raising); the dispatch orchestration
/// lives here, not on the token.
///
/// The helper runs `kobako.mrb().get_args::<NRestBlock>()` itself, so
/// callers must not have already consumed the arglist.
fn forward_to_dispatch(
    kobako: super::Kobako,
    target: crate::transport::Target,
    sym_err_msg: &core::ffi::CStr,
    envelope_err_msg: &core::ffi::CStr,
) -> Value {
    use crate::abi::block_stack::BlockFrame;
    use crate::transport::proxy::{invoke, InvokeError};

    let (method_sym, rest, block) = kobako.mrb().get_args::<crate::mruby::format::NRestBlock>();

    // Push the block onto BLOCK_STACK for the duration of this bridge
    // frame; drops + pops automatically on return / mruby raise. The
    // wire-level `block_given` bit (B-23) is the observable shadow of
    // the same fact.
    let block_given = !block.is_nil();
    let _block_frame = BlockFrame::push_if_block(block);

    let method_name = match kobako.mrb().sym_name(method_sym) {
        Some(name) => name,
        None => unsafe { kobako.raise_transport_error(sym_err_msg) },
    };

    let (args, kwargs) = kobako.unpack_args_kwargs(rest);

    match invoke(target, method_name, &args, &kwargs, block_given) {
        Ok(value) => kobako.to_mrb_value(value),
        // SAFETY: bridge frame — mruby unwinds through `mrb_raise`.
        Err(InvokeError::Service(ex)) => unsafe { kobako.raise_service_error(&ex) },
        // SAFETY: as above.
        Err(_) => unsafe { kobako.raise_transport_error(envelope_err_msg) },
    }
}

/// `Kobako::Member.method_missing(name, *args)` C bridge —
/// singleton-class level, so `self` is the Member class object (e.g.
/// `MyService::KV`).
///
/// Extracts:
///   - `target` = full class name via `mrb_class_name(mrb_class_ptr(self))`
///   - `method` = first arg (Symbol → String)
///   - `args`   = rest args (positional), last arg absorbed into kwargs if Hash
///   - `kwargs` = trailing Hash arg (if last positional is a Hash)
///
/// Forwards to `forward_to_dispatch` with `Target::Path`.
pub(crate) unsafe extern "C" fn member_method_missing(
    mrb: *mut sys::mrb_state,
    self_: Value,
) -> Value {
    use crate::transport::Target;

    // SAFETY: bridge contract.
    let kobako = unsafe { super::Kobako::resolve_raw(mrb) };

    // SAFETY: `self_` is the class receiver of a singleton-class
    // `method_missing` shim — class-tagged by mruby itself.
    let class = crate::mruby::Class::from_raw(unsafe { self_.as_class_ptr() });
    let target_str = match class.name(kobako.mrb()) {
        Some(name) => name,
        None => unsafe {
            // SAFETY: bridge frame.
            kobako.raise_transport_error(c"transport target class name is null")
        },
    };
    let target = Target::Path(target_str.to_string());

    forward_to_dispatch(
        kobako,
        target,
        c"Member method symbol name is null",
        c"transport envelope error (Member dispatch)",
    )
}

/// `Kobako::Member.new` / `.allocate` C bridge — singleton-class level.
/// A Member is a dispatch target, never instantiated by guest code
/// (docs/behavior.md B-38), so both construction entries raise
/// `NoMethodError` naming the offending Member rather than producing an
/// inert empty instance. Registered with `mrb_args_any()` so the raise
/// fires regardless of arguments instead of tripping an arity check first.
pub(crate) unsafe extern "C" fn member_not_constructible(
    mrb: *mut sys::mrb_state,
    self_: Value,
) -> Value {
    // SAFETY: bridge contract — `mrb` is live for the call.
    let mrb_ref = unsafe { crate::mruby::Mrb::borrow_raw(&mrb) };
    let nomethod = mrb_ref.class_get(c"NoMethodError");
    // SAFETY: `self_` is the Member class receiver of a singleton-class
    // method — class-tagged by mruby itself.
    let class = crate::mruby::Class::from_raw(unsafe { self_.as_class_ptr() });
    let message = match class.name(mrb_ref) {
        Some(name) => std::ffi::CString::new(format!(
            "{name} is a Kobako Member (a dispatch target), not a constructible class"
        ))
        .unwrap_or_default(),
        None => c"Kobako Member is not constructible".to_owned(),
    };
    // SAFETY: bridge frame — mruby unwinds through `mrb_raise`.
    unsafe { nomethod.raise(mrb_ref, &message) }
}

/// `Kobako::Handle#initialize(id)` C bridge. Stores the Handle integer
/// id into the `@__kobako_id__` instance variable via
/// `super::Kobako::set_handle_id`.
pub(crate) unsafe extern "C" fn handle_initialize(mrb: *mut sys::mrb_state, self_: Value) -> Value {
    // SAFETY: bridge contract.
    let kobako = unsafe { super::Kobako::resolve_raw(mrb) };
    let id_val = kobako.mrb().get_args::<crate::mruby::format::O>();
    kobako.set_handle_id(self_, id_val);
    Value::zeroed()
}

/// `Kobako::Handle#method_missing(name, *args)` C bridge — instance
/// level, so `self` is a `Kobako::Handle` instance. Derives
/// `Target::Handle(handle_id)` from the receiver's `@__kobako_id__` ivar
/// — the Handle chaining path (docs/behavior.md B-17). The Handle
/// carries only that id; all of its dispatch behaviour is this one
/// method plus the inherited `forward_to_dispatch` body.
///
/// Forwards to `forward_to_dispatch` with `Target::Handle`.
pub(crate) unsafe extern "C" fn handle_method_missing(
    mrb: *mut sys::mrb_state,
    self_: Value,
) -> Value {
    use crate::transport::Target;

    // SAFETY: bridge contract.
    let kobako = unsafe { super::Kobako::resolve_raw(mrb) };
    let handle_id = kobako.extract_handle_id(self_);
    let target = Target::Handle(handle_id);

    forward_to_dispatch(
        kobako,
        target,
        c"Handle method symbol name is null",
        c"transport envelope error (Handle dispatch)",
    )
}

/// `respond_to_missing?(name, include_private)` C bridge, shared by
/// `Kobako::Member` and `Kobako::Handle`. Always returns `true` — every
/// method call is dispatched through `method_missing` to the host, so
/// probing via `respond_to?` must succeed (docs/behavior.md B-36).
/// Registered singleton-class on `Kobako::Member` (Member classes) and
/// instance-class on `Kobako::Handle`.
pub(crate) unsafe extern "C" fn proxy_respond_to_missing(
    _mrb: *mut sys::mrb_state,
    _self_: Value,
) -> Value {
    // No VM access needed: `Value::true_()` reads the sys-side immediates
    // cache, populated at install before any probe runs, so the raw
    // `mrb` pointer goes unused.
    Value::true_()
}
