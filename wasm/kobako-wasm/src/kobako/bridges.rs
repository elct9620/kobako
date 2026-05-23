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
//!        │  through to singleton-class `method_missing`, inherited
//!        │  from `Kobako::Transport::Proxy.singleton_class`)
//!        ▼
//!   transport_proxy_method_missing(mrb, self=KV.class)
//!        │
//!        │ (extract method symbol + args; build kwargs hash from
//!        │  trailing Hash if present; resolve target string via
//!        │  `mrb_class_name(mrb, mrb_class_ptr(self))`)
//!        ▼
//!   super::Kobako::dispatch_invoke(target, method, args, kwargs)
//!        │
//!        ▼
//!   crate::rpc::client::invoke_rpc(...)
//! ```
//!
//! Handle dispatch (`Kobako::Handle#method_missing`, docs/behavior.md B-17)
//! follows the same shape: `handle_method_missing` builds a Handle
//! target and calls `dispatch_invoke` directly. Both bridges share the
//! same Rust helper (`Kobako::dispatch_invoke`) — only the `Target`
//! variant they construct differs.
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

/// `Kobako::Transport::Proxy.method_missing(name, *args)` C bridge —
/// singleton-class level, so `self` is the class object (e.g.
/// `MyService::KV`).
///
/// Extracts:
///   - `target` = full class name via `mrb_class_name(mrb_class_ptr(self))`
///   - `method` = first arg (Symbol → String)
///   - `args`   = rest args (positional), last arg absorbed into kwargs if Hash
///   - `kwargs` = trailing Hash arg (if last positional is a Hash)
///
/// Forwards to [`super::Kobako::dispatch_invoke`].
pub(crate) unsafe extern "C" fn transport_proxy_method_missing(
    mrb: *mut sys::mrb_state,
    self_: Value,
) -> Value {
    #[cfg(target_arch = "wasm32")]
    {
        use crate::abi::block_stack::BlockFrame;
        use crate::rpc::envelope::Target;

        // SAFETY: bridge contract.
        let kobako = unsafe { super::Kobako::resolve_raw(mrb) };
        let (method_sym, rest, block) = kobako.mrb().get_args::<sys::format::NRestBlock>();

        // Push the block onto BLOCK_STACK for the duration of this
        // bridge frame; drops + pops automatically on return / mruby
        // raise. The wire-level `block_given` bit (B-23) is the
        // observable shadow of the same fact.
        let block_given = !block.is_nil();
        let _block_frame = BlockFrame::push_if_block(block);

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

        let method_name = match kobako.mrb().sym_name(method_sym) {
            Some(name) => name,
            None => unsafe { kobako.raise_wire_error(c"transport method symbol name is null") },
        };

        let (args, kwargs) = kobako.unpack_args_kwargs(rest);
        let target = Target::Path(target_str.to_string());

        kobako.dispatch_invoke(
            target,
            method_name,
            &args,
            &kwargs,
            block_given,
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
        // Host stub — see `transport_proxy_method_missing` for the shape rationale.
        let _ = mrb;
        let _ = self_;
    }
    Value::zeroed()
}

/// `Kobako::Handle#method_missing(name, *args)` C bridge. Forwards every
/// method call on a Handle instance to the host via
/// [`super::Kobako::dispatch_invoke`] with the Handle id as the target
/// (docs/behavior.md B-17 — Handle chaining).
///
/// TODO: Phase 2 originally intended to absorb this bridge into
/// `transport_proxy_method_missing` so Handle becomes a pure value
/// type with no `method_missing`. Keeping both bridges for now —
/// they already share `Kobako::dispatch_invoke`, only the +Target+
/// variant differs — to limit Phase 2 blast radius. Revisit once
/// Phase 3's Runtime / Invocation slot work lands.
pub(crate) unsafe extern "C" fn handle_method_missing(
    mrb: *mut sys::mrb_state,
    self_: Value,
) -> Value {
    #[cfg(target_arch = "wasm32")]
    {
        use crate::abi::block_stack::BlockFrame;
        use crate::rpc::envelope::Target;

        // SAFETY: bridge contract.
        let kobako = unsafe { super::Kobako::resolve_raw(mrb) };
        let (method_sym, rest, block) = kobako.mrb().get_args::<sys::format::NRestBlock>();

        // See `transport_proxy_method_missing` for the BLOCK_STACK /
        // block_given rationale; same shape applies to Handle dispatch
        // (B-17).
        let block_given = !block.is_nil();
        let _block_frame = BlockFrame::push_if_block(block);

        let handle_id = kobako.extract_handle_id(self_);

        let method_name = match kobako.mrb().sym_name(method_sym) {
            Some(name) => name,
            None => unsafe { kobako.raise_wire_error(c"Handle method symbol name is null") },
        };

        let (args, kwargs) = kobako.unpack_args_kwargs(rest);
        let target = Target::Handle(handle_id);

        kobako.dispatch_invoke(
            target,
            method_name,
            &args,
            &kwargs,
            block_given,
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

/// `Kobako::Transport::Proxy.respond_to_missing?(name, include_private)`
/// C bridge. Always returns `true` — every method call on a Member
/// class is dispatched through `method_missing` to the host, so probing
/// via `respond_to?` must succeed. Also registered on `Kobako::Handle`
/// for the same reason (B-17 Handle chaining).
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
        // Host stub — see `transport_proxy_method_missing` for the shape rationale.
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
        let _f1: sys::mrb_func_t = transport_proxy_method_missing;
        let _f2: sys::mrb_func_t = transport_proxy_respond_to_missing;
        let _f3: sys::mrb_func_t = handle_initialize;
        let _f4: sys::mrb_func_t = handle_method_missing;
    }
}
