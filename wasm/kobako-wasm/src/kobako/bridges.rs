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
//!        │  from `Kobako::RPC::Client.singleton_class`)
//!        ▼
//!   rpc_method_missing(mrb, self=KV.class)
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
//! Handle dispatch (`Kobako::RPC::Handle#method_missing`, docs/behavior.md B-17)
//! follows the same shape: `handle_method_missing` builds a Handle
//! target and calls `dispatch_invoke` directly.
//!
//! ## Safety
//!
//! Each bridge is `unsafe extern "C" fn` because mruby invokes it from
//! the C side with a raw `*mut mrb_state` and a `Value` receiver.
//! Bodies open with `unsafe { Kobako::resolve_raw(mrb) }` to obtain the
//! safe `Kobako` token; from then on the work is safe Rust with
//! explicit `unsafe { ... }` blocks at each remaining FFI call site.

#[cfg(target_arch = "wasm32")]
use crate::cstr;
use crate::mruby::sys;
use crate::mruby::sys::Value;

/// Build a borrowed `&[Value]` from a `mrb_get_args("n*", ...)`
/// out-parameter pair. mruby may set `rest_ptr` to NULL when the
/// rest-array is empty; reading `rest_len` bytes from a NULL pointer
/// would be UB, so the helper returns an empty slice in that case.
///
/// `rest_ptr` arrives typed as `*const sys::mrb_value` (matching
/// mruby's variadic out-param contract); the cast to `*const Value`
/// is sound because `Value` is `#[repr(transparent)]` over
/// `mrb_value`.
///
/// # Safety
///
/// When `rest_len > 0`, `rest_ptr` must point to a contiguous array
/// of `rest_len` `mrb_value` entries produced by `mrb_get_args` on
/// the current call frame. The slice borrows from that array and
/// must not outlive the call.
#[cfg(target_arch = "wasm32")]
unsafe fn slice_from_mrb_args<'a>(
    rest_ptr: *const sys::mrb_value,
    rest_len: core::ffi::c_int,
) -> &'a [Value] {
    if rest_len > 0 && !rest_ptr.is_null() {
        // SAFETY: see item-level doc. Cast through `*const Value` is
        // sound by `repr(transparent)`.
        unsafe { core::slice::from_raw_parts(rest_ptr as *const Value, rest_len as usize) }
    } else {
        &[]
    }
}

/// `Kobako::RPC.method_missing(name, *args)` C bridge — singleton-class
/// level, so `self` is the class object (e.g. `MyService::KV`).
///
/// Extracts:
///   - `target` = full class name via `mrb_class_name(mrb_class_ptr(self))`
///   - `method` = first arg (Symbol → String)
///   - `args`   = rest args (positional), last arg absorbed into kwargs if Hash
///   - `kwargs` = trailing Hash arg (if last positional is a Hash)
///
/// Forwards to [`super::Kobako::dispatch_invoke`].
pub(crate) unsafe extern "C" fn rpc_method_missing(
    mrb: *mut sys::mrb_state,
    self_: Value,
) -> Value {
    #[cfg(target_arch = "wasm32")]
    {
        use crate::rpc::envelope::Target;

        // SAFETY: bridge contract.
        let kobako = unsafe { super::Kobako::resolve_raw(mrb) };

        let mut method_sym: sys::mrb_sym = 0;
        let mut rest_ptr: *const sys::mrb_value = core::ptr::null();
        let mut rest_len: core::ffi::c_int = 0;
        unsafe {
            sys::mrb_get_args(
                mrb,
                cstr!("n*"),
                &mut method_sym as *mut sys::mrb_sym,
                &mut rest_ptr as *mut *const sys::mrb_value,
                &mut rest_len as *mut core::ffi::c_int,
            );
        }

        // SAFETY: `self_` is the class receiver of a singleton-class
        // `method_missing` shim — class-tagged by mruby itself.
        let class_ptr = unsafe { self_.as_class_ptr() };
        let class_name_ptr = unsafe { sys::mrb_class_name(mrb, class_ptr) };
        let target_str = if class_name_ptr.is_null() {
            // SAFETY: bridge frame.
            unsafe { kobako.raise_wire_error(b"RPC target class name is null\0") };
        } else {
            unsafe { core::ffi::CStr::from_ptr(class_name_ptr) }
                .to_str()
                .unwrap_or("")
        };

        let method_name_ptr = unsafe { sys::mrb_sym_name(mrb, method_sym) };
        let method_name = if method_name_ptr.is_null() {
            unsafe { kobako.raise_wire_error(b"RPC method symbol name is null\0") };
        } else {
            unsafe { core::ffi::CStr::from_ptr(method_name_ptr) }
                .to_str()
                .unwrap_or("")
        };

        // SAFETY: mruby passes a valid array on `n*` unpack; see
        // [`slice_from_mrb_args`] for the empty-slice handling.
        let rest = unsafe { slice_from_mrb_args(rest_ptr, rest_len) };

        let (wire_args, wire_kwargs) = kobako.unpack_args_kwargs(rest);
        let target = Target::Path(target_str.to_string());

        kobako.dispatch_invoke(
            target,
            method_name,
            &wire_args,
            &wire_kwargs,
            b"RPC wire error\0",
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

/// `Kobako::RPC::Handle#initialize(id)` C bridge. Stores the Handle integer
/// id into the `@__kobako_id__` instance variable via
/// [`super::Kobako::set_handle_id`].
pub(crate) unsafe extern "C" fn handle_initialize(mrb: *mut sys::mrb_state, self_: Value) -> Value {
    #[cfg(target_arch = "wasm32")]
    {
        // SAFETY: bridge contract.
        let kobako = unsafe { super::Kobako::resolve_raw(mrb) };

        // mrb_get_args writes its `"o"` slot as a raw mrb_value; wrap
        // the cell once retrieved.
        let mut id_raw = sys::mrb_value::zeroed();
        unsafe {
            sys::mrb_get_args(mrb, cstr!("o"), &mut id_raw as *mut sys::mrb_value);
        }
        kobako.set_handle_id(self_, Value::from_raw(id_raw));
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        // Host stub — see `rpc_method_missing` for the shape rationale.
        let _ = mrb;
        let _ = self_;
    }
    Value::zeroed()
}

/// `Kobako::RPC::Handle#method_missing(name, *args)` C bridge. Forwards every
/// method call on a Handle instance to the host via
/// [`super::Kobako::dispatch_invoke`] with the Handle id as the target
/// (docs/behavior.md B-17 — Handle chaining).
pub(crate) unsafe extern "C" fn handle_method_missing(
    mrb: *mut sys::mrb_state,
    self_: Value,
) -> Value {
    #[cfg(target_arch = "wasm32")]
    {
        use crate::rpc::envelope::Target;

        // SAFETY: bridge contract.
        let kobako = unsafe { super::Kobako::resolve_raw(mrb) };

        let mut method_sym: sys::mrb_sym = 0;
        let mut rest_ptr: *const sys::mrb_value = core::ptr::null();
        let mut rest_len: core::ffi::c_int = 0;
        unsafe {
            sys::mrb_get_args(
                mrb,
                cstr!("n*"),
                &mut method_sym as *mut sys::mrb_sym,
                &mut rest_ptr as *mut *const sys::mrb_value,
                &mut rest_len as *mut core::ffi::c_int,
            );
        }

        let handle_id = kobako.extract_handle_id(self_);

        let method_name_ptr = unsafe { sys::mrb_sym_name(mrb, method_sym) };
        let method_name = if method_name_ptr.is_null() {
            unsafe { kobako.raise_wire_error(b"Handle method symbol name is null\0") };
        } else {
            unsafe { core::ffi::CStr::from_ptr(method_name_ptr) }
                .to_str()
                .unwrap_or("")
        };

        // SAFETY: see [`slice_from_mrb_args`].
        let rest = unsafe { slice_from_mrb_args(rest_ptr, rest_len) };

        let (wire_args, wire_kwargs) = kobako.unpack_args_kwargs(rest);
        let target = Target::Handle(handle_id);

        kobako.dispatch_invoke(
            target,
            method_name,
            &wire_args,
            &wire_kwargs,
            b"RPC wire error (Handle dispatch)\0",
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

/// `Kobako::RPC.respond_to_missing?(name, include_private)` C bridge.
/// Always returns `true` — every method call on a Member class
/// is dispatched through `method_missing` to the host, so probing via
/// `respond_to?` must succeed.
pub(crate) unsafe extern "C" fn rpc_respond_to_missing(
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
        // Host stub — see `rpc_method_missing` for the shape rationale.
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
        let _f1: sys::mrb_func_t = rpc_method_missing;
        let _f2: sys::mrb_func_t = rpc_respond_to_missing;
        let _f3: sys::mrb_func_t = handle_initialize;
        let _f4: sys::mrb_func_t = handle_method_missing;
    }
}
