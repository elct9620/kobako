//! C-callable shims registered with mruby at install time.
//!
//! Every function here matches the [`crate::mruby::sys::mrb_func_t`]
//! signature so mruby can invoke it as a method body. The registrations
//! happen in [`super::Kobako::install_raw`]; the bridges themselves
//! re-enter the boundary by resolving a `Kobako` token via
//! [`super::Kobako::resolve_raw`] and then call safe methods.
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
//!   super::Kobako::dispatch_invoke(target, method, args, kwargs)
//!        ▼
//!   crate::transport::proxy::invoke(...)
//! ```
//!
//! The two `method_missing` bridges live on the two `Kobako::Transport::Proxy`
//! subclasses: [`member_method_missing`] is the singleton-class shim on
//! `Kobako::Member` (Member *classes*, `Target::Path`) and
//! [`handle_method_missing`] is the instance shim on `Kobako::Handle`
//! (Handle *instances*, `Target::Handle`, docs/behavior.md B-17). The
//! two differ only in how they derive the `Target` from `self_`; the
//! BlockFrame push, method-symbol extraction, args/kwargs unpacking, and
//! `dispatch_invoke` call all live in [`forward_to_dispatch`].
//!
//! ## Safety
//!
//! Each bridge is `unsafe extern "C" fn` because mruby invokes it from
//! the C side with a raw `*mut mrb_state` and a `Value` receiver.
//! Bodies open with `unsafe { Kobako::resolve_raw(mrb) }` to obtain the
//! safe `Kobako` token; from then on the work is safe Rust with
//! explicit `unsafe { ... }` blocks at each remaining FFI call site.

use crate::mruby::sys;
use crate::mruby::sys::Value;

/// Shared body for the two `method_missing` bridges. The caller
/// supplies the `Target` it derived from its `self_` receiver (a class
/// name for the `Kobako::Member` singleton-class shim, a Handle id for
/// the `Kobako::Handle` instance shim) plus the error label that flows
/// into a wire-error
/// raise on a failed dispatch. Everything else — BlockFrame push,
/// method-symbol extraction, args/kwargs unpacking, the
/// [`super::Kobako::dispatch_invoke`] call — is identical for both
/// bridges and lives here.
///
/// The helper runs `kobako.mrb().get_args::<NRestBlock>()` itself, so
/// callers must not have already consumed the arglist.
#[cfg(target_arch = "wasm32")]
fn forward_to_dispatch(
    kobako: super::Kobako,
    target: crate::transport::envelope::Target,
    sym_err_msg: &core::ffi::CStr,
    envelope_err_msg: &core::ffi::CStr,
) -> Value {
    use crate::abi::block_stack::BlockFrame;

    let (method_sym, rest, block) = kobako.mrb().get_args::<sys::format::NRestBlock>();

    // Push the block onto BLOCK_STACK for the duration of this bridge
    // frame; drops + pops automatically on return / mruby raise. The
    // wire-level `block_given` bit (B-23) is the observable shadow of
    // the same fact.
    let block_given = !block.is_nil();
    let _block_frame = BlockFrame::push_if_block(block);

    let method_name = match kobako.mrb().sym_name(method_sym) {
        Some(name) => name,
        None => unsafe { kobako.raise_wire_error(sym_err_msg) },
    };

    let (args, kwargs) = kobako.unpack_args_kwargs(rest);

    kobako.dispatch_invoke(
        target,
        method_name,
        &args,
        &kwargs,
        block_given,
        envelope_err_msg,
    )
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
/// Forwards to [`forward_to_dispatch`] with `Target::Path`.
pub(crate) unsafe extern "C" fn member_method_missing(
    mrb: *mut sys::mrb_state,
    self_: Value,
) -> Value {
    #[cfg(target_arch = "wasm32")]
    {
        use crate::transport::envelope::Target;

        // SAFETY: bridge contract.
        let kobako = unsafe { super::Kobako::resolve_raw(mrb) };

        // SAFETY: `self_` is the class receiver of a singleton-class
        // `method_missing` shim — class-tagged by mruby itself.
        let class = sys::Class::from_raw(unsafe { self_.as_class_ptr() });
        let target_str = match class.name(kobako.mrb()) {
            Some(name) => name,
            None => unsafe {
                // SAFETY: bridge frame.
                kobako.raise_wire_error(c"transport target class name is null")
            },
        };
        let target = Target::Path(target_str.to_string());

        forward_to_dispatch(
            kobako,
            target,
            c"transport method symbol name is null",
            c"transport envelope error",
        )
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        // Host stub — mrb_func_t shape must keep the params even when
        // the body is wasm32-only; consume the bindings locally so the
        // `unused_variables` lint is satisfied without an `#[allow]`.
        let _ = mrb;
        let _ = self_;
        Value::zeroed()
    }
}

/// `Kobako::Handle#initialize(id)` C bridge. Stores the Handle integer
/// id into the `@__kobako_id__` instance variable via
/// [`super::Kobako::set_handle_id`].
pub(crate) unsafe extern "C" fn handle_initialize(mrb: *mut sys::mrb_state, self_: Value) -> Value {
    #[cfg(target_arch = "wasm32")]
    {
        // SAFETY: bridge contract.
        let kobako = unsafe { super::Kobako::resolve_raw(mrb) };
        let id_val = kobako.mrb().get_args::<sys::format::O>();
        kobako.set_handle_id(self_, id_val);
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        // Host stub — see `member_method_missing` for the shape rationale.
        let _ = mrb;
        let _ = self_;
    }
    Value::zeroed()
}

/// `Kobako::Handle#method_missing(name, *args)` C bridge — instance
/// level, so `self` is a `Kobako::Handle` instance. Derives
/// `Target::Handle(handle_id)` from the receiver's `@__kobako_id__` ivar
/// — the Handle chaining path (docs/behavior.md B-17). The Handle
/// carries only that id; all of its dispatch behaviour is this one
/// method plus the inherited `forward_to_dispatch` body.
///
/// Forwards to [`forward_to_dispatch`] with `Target::Handle`.
pub(crate) unsafe extern "C" fn handle_method_missing(
    mrb: *mut sys::mrb_state,
    self_: Value,
) -> Value {
    #[cfg(target_arch = "wasm32")]
    {
        use crate::transport::envelope::Target;

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
    #[cfg(not(target_arch = "wasm32"))]
    {
        // Host stub — mrb_func_t shape must keep the params even when
        // the body is wasm32-only; consume the bindings locally so the
        // `unused_variables` lint is satisfied without an `#[allow]`.
        let _ = mrb;
        let _ = self_;
        Value::zeroed()
    }
}

/// `respond_to_missing?(name, include_private)` C bridge, shared by
/// `Kobako::Member` and `Kobako::Handle`. Always returns `true` — every
/// method call is dispatched through `method_missing` to the host, so
/// probing via `respond_to?` must succeed. Registered singleton-class
/// on `Kobako::Member` (Member classes) and instance-class on
/// `Kobako::Handle` for the same reason (B-17 Handle chaining).
pub(crate) unsafe extern "C" fn transport_proxy_respond_to_missing(
    mrb: *mut sys::mrb_state,
    _self_: Value,
) -> Value {
    #[cfg(target_arch = "wasm32")]
    {
        // SAFETY: bridge contract — resolve_raw needed only to assert
        // the install precondition; the immediate `true` is sourced
        // from the sys-side cache directly.
        let _kobako = unsafe { super::Kobako::resolve_raw(mrb) };
        Value::true_()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        // Host stub — see `member_method_missing` for the shape rationale.
        let _ = mrb;
        Value::zeroed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn c_bridges_have_mrb_func_t_signature() {
        // Compile-time signature check — these `let` bindings fail to
        // compile if the bridge functions drift from `mrb_func_t`. This
        // is the host-target replacement for an mruby-link-level
        // signature check.
        let _f1: sys::mrb_func_t = member_method_missing;
        let _f2: sys::mrb_func_t = transport_proxy_respond_to_missing;
        let _f3: sys::mrb_func_t = handle_initialize;
        let _f4: sys::mrb_func_t = handle_method_missing;
    }
}
