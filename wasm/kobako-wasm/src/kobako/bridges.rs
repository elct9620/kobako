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
//! Handle dispatch (`Kobako::RPC::Handle#method_missing`, SPEC.md B-17)
//! follows the same shape: `handle_method_missing` builds a Handle
//! target and calls `dispatch_invoke` directly.
//!
//! ## Safety
//!
//! Each bridge is `unsafe extern "C" fn` because mruby invokes it from
//! the C side with a raw `*mut mrb_state` and an `mrb_value` receiver.
//! Bodies open with `unsafe { Kobako::resolve_raw(mrb) }` to obtain the
//! safe `Kobako` token; from then on the work is safe Rust with
//! explicit `unsafe { ... }` blocks at each remaining FFI call site.

#[cfg(target_arch = "wasm32")]
use crate::cstr;
use crate::mruby::sys;

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
#[allow(unused_variables)]
pub(crate) unsafe extern "C" fn rpc_method_missing(
    mrb: *mut sys::mrb_state,
    self_: sys::mrb_value,
) -> sys::mrb_value {
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

        // Get the target class name. `mrb_class_ptr(val)` is the C macro
        // `((struct RClass*)(mrb_ptr(val)))`. On wasm32 with
        // MRB_WORDBOX_NO_INLINE_FLOAT + MRB_INT32, `mrb_ptr(val)` is the
        // raw pointer stored in the lower bits of `mrb_value.w` for
        // object-tagged values — i.e. `self_.w as *mut RClass`. We
        // implement the macro inline to avoid declaring it as an extern
        // "C" fn (it is a macro, not a function, so an FFI declaration
        // would produce an unresolved wasm import).
        let class_ptr = self_.w as *mut sys::RClass;
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

        let rest: &[sys::mrb_value] = if rest_len > 0 && !rest_ptr.is_null() {
            // SAFETY: mruby passes a valid array on `n*` unpack.
            unsafe { core::slice::from_raw_parts(rest_ptr, rest_len as usize) }
        } else {
            &[]
        };

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
        sys::mrb_value::zeroed()
    }
}

/// `Kobako::RPC::Handle#initialize(id)` C bridge. Stores the Handle integer
/// id into the `@__kobako_id__` instance variable via
/// [`super::Kobako::set_handle_id`].
#[allow(unused_variables)]
pub(crate) unsafe extern "C" fn handle_initialize(
    mrb: *mut sys::mrb_state,
    self_: sys::mrb_value,
) -> sys::mrb_value {
    #[cfg(target_arch = "wasm32")]
    {
        // SAFETY: bridge contract.
        let kobako = unsafe { super::Kobako::resolve_raw(mrb) };

        let mut id_val = sys::mrb_value::zeroed();
        unsafe {
            sys::mrb_get_args(mrb, cstr!("o"), &mut id_val as *mut sys::mrb_value);
        }
        kobako.set_handle_id(self_, id_val);
    }
    sys::mrb_value::zeroed()
}

/// `Kobako::RPC::Handle#method_missing(name, *args)` C bridge. Forwards every
/// method call on a Handle instance to the host via
/// [`super::Kobako::dispatch_invoke`] with the Handle id as the target
/// (SPEC.md B-17 — Handle chaining).
#[allow(unused_variables)]
pub(crate) unsafe extern "C" fn handle_method_missing(
    mrb: *mut sys::mrb_state,
    self_: sys::mrb_value,
) -> sys::mrb_value {
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

        let rest: &[sys::mrb_value] = if rest_len > 0 && !rest_ptr.is_null() {
            unsafe { core::slice::from_raw_parts(rest_ptr, rest_len as usize) }
        } else {
            &[]
        };

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
        sys::mrb_value::zeroed()
    }
}

/// `Kobako::RPC.respond_to_missing?(name, include_private)` C bridge.
/// Always returns `true` — every method call on a Service Member class
/// is dispatched through `method_missing` to the host, so probing via
/// `respond_to?` must succeed.
#[allow(unused_variables)]
pub(crate) unsafe extern "C" fn rpc_respond_to_missing(
    mrb: *mut sys::mrb_state,
    _self_: sys::mrb_value,
) -> sys::mrb_value {
    #[cfg(target_arch = "wasm32")]
    {
        // SAFETY: bridge contract.
        let kobako = unsafe { super::Kobako::resolve_raw(mrb) };
        kobako.true_value()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        sys::mrb_value::zeroed()
    }
}

// --------------------------------------------------------------------
// `Kernel#puts` / `Kernel#p` C bridges.
// --------------------------------------------------------------------
//
// Both bridges delegate to `Kernel#print` via `mrb_funcall`. We take
// the `mrb_funcall` route rather than calling mruby's internal
// `mrb_print_m` directly because `mrb_print_m` is not part of the
// public mruby C API (it's a static handler installed in `kernel.c`).
//
// `puts` semantics matched against MRI:
//   * No args             → emit a single "\n".
//   * `Array` arg         → recurse into each element (one line per
//                            non-Array element, preserving order).
//   * Other args          → call `.to_s`, print, append "\n" unless the
//                            string already ends in "\n".
//
// `p` semantics matched against MRI:
//   * Each arg            → print `arg.inspect` followed by "\n".
//   * Returns the single arg if `argc == 1`, the args Array otherwise,
//     `nil` when called with no arguments.

/// `Kernel#puts(*args)` C bridge.
#[allow(unused_variables)]
pub(crate) unsafe extern "C" fn kernel_puts(
    mrb: *mut sys::mrb_state,
    self_: sys::mrb_value,
) -> sys::mrb_value {
    #[cfg(target_arch = "wasm32")]
    {
        // SAFETY: bridge contract.
        let kobako = unsafe { super::Kobako::resolve_raw(mrb) };

        let mut args_ptr: *const sys::mrb_value = core::ptr::null();
        let mut args_len: core::ffi::c_int = 0;
        unsafe {
            sys::mrb_get_args(
                mrb,
                cstr!("*"),
                &mut args_ptr as *mut *const sys::mrb_value,
                &mut args_len as *mut core::ffi::c_int,
            );
        }

        let nl = unsafe { sys::mrb_str_new(mrb, b"\n".as_ptr() as *const core::ffi::c_char, 1) };

        if args_len == 0 {
            kobako.print_str(self_, nl);
            return kobako.nil_value();
        }

        let args = unsafe { core::slice::from_raw_parts(args_ptr, args_len as usize) };
        for &arg in args {
            kobako.puts_one(self_, arg, nl);
        }
        kobako.nil_value()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        sys::mrb_value::zeroed()
    }
}

/// `Kernel#p(*args)` C bridge.
#[allow(unused_variables)]
pub(crate) unsafe extern "C" fn kernel_p(
    mrb: *mut sys::mrb_state,
    self_: sys::mrb_value,
) -> sys::mrb_value {
    #[cfg(target_arch = "wasm32")]
    {
        // SAFETY: bridge contract.
        let kobako = unsafe { super::Kobako::resolve_raw(mrb) };

        let mut args_ptr: *const sys::mrb_value = core::ptr::null();
        let mut args_len: core::ffi::c_int = 0;
        unsafe {
            sys::mrb_get_args(
                mrb,
                cstr!("*"),
                &mut args_ptr as *mut *const sys::mrb_value,
                &mut args_len as *mut core::ffi::c_int,
            );
        }

        let nl = unsafe { sys::mrb_str_new(mrb, b"\n".as_ptr() as *const core::ffi::c_char, 1) };
        let args = unsafe { core::slice::from_raw_parts(args_ptr, args_len as usize) };
        for &arg in args {
            let insp = unsafe { arg.call(mrb, cstr!("inspect"), &[]) };
            kobako.print_str(self_, insp);
            kobako.print_str(self_, nl);
        }

        match args_len {
            0 => kobako.nil_value(),
            1 => args[0],
            _ => unsafe { sys::mrb_ary_new_from_values(mrb, args_len, args_ptr) },
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        sys::mrb_value::zeroed()
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
        let _f5: sys::mrb_func_t = kernel_puts;
        let _f6: sys::mrb_func_t = kernel_p;
    }
}
