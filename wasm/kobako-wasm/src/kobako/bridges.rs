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
//!        │  from `Kobako::RPC.singleton_class`)
//!        ▼
//!   rpc_method_missing(mrb, self=KV.class)
//!        │
//!        │ (extract method symbol + args; build kwargs hash from
//!        │  trailing Hash if present; resolve target string via
//!        │  `mrb_class_name(mrb, mrb_class_ptr(self))`)
//!        ▼
//!   forwards to Kobako.__rpc_call__(target, method, args, kwargs)
//!        │
//!        ▼
//!   kobako_rpc_call(mrb, self=Kobako module)
//!        │
//!        ▼
//!   crate::rpc_client::invoke_rpc(...)
//! ```
//!
//! ## Safety
//!
//! Each bridge is `unsafe extern "C" fn` because mruby invokes it from
//! the C side with a raw `*mut mrb_state` and an `mrb_value` receiver.
//! Bodies open with `unsafe { Kobako::resolve_raw(mrb) }` to obtain the
//! safe `Kobako` token; from then on the work is safe Rust with
//! explicit `unsafe { ... }` blocks at each remaining FFI call site.

use crate::cstr;
use crate::mruby::sys;

/// `Kobako.__rpc_call__(target, method, args, kwargs)` C bridge.
///
/// Receives four positional args assembled by [`rpc_method_missing`]:
///   - `target`: String path (e.g. `"MyService::KV"`) or `Kobako::Handle` instance.
///   - `method`: String method name.
///   - `args`: Array of positional Wire values.
///   - `kwargs`: Hash of String key → Wire value pairs.
///
/// Delegates to [`crate::rpc_client::invoke_rpc`] through
/// [`super::Kobako::dispatch_invoke`]. On success, returns the
/// wire-decoded mruby value; on service error, raises
/// `Kobako::ServiceError`; on wire error, raises `Kobako::WireError`.
#[allow(unused_variables)]
pub(crate) unsafe extern "C" fn kobako_rpc_call(
    mrb: *mut sys::mrb_state,
    _self_: sys::mrb_value,
) -> sys::mrb_value {
    #[cfg(target_arch = "wasm32")]
    {
        use crate::envelope::Target;

        // SAFETY: `mrb` is live by the bridge contract (mruby invoked us
        // through a registration done at install time). Construct a
        // `Kobako` token once at the top — everything below uses safe
        // methods.
        let kobako = unsafe { super::Kobako::resolve_raw(mrb) };

        // Unpack 4 required positional args: target, method, args_ary, kwargs_hash.
        let mut target_val = sys::mrb_value::zeroed();
        let mut method_val = sys::mrb_value::zeroed();
        let mut args_ary = sys::mrb_value::zeroed();
        let mut kwargs_hash = sys::mrb_value::zeroed();
        unsafe {
            sys::mrb_get_args(
                mrb,
                cstr!("oooo"),
                &mut target_val as *mut sys::mrb_value,
                &mut method_val as *mut sys::mrb_value,
                &mut args_ary as *mut sys::mrb_value,
                &mut kwargs_hash as *mut sys::mrb_value,
            );
        }

        // Decode target: String path or Kobako::Handle instance.
        let target = match unsafe { target_val.classname(mrb) } {
            "Kobako::Handle" => Target::Handle(kobako.extract_handle_id(target_val)),
            _ => Target::Path(unsafe { target_val.to_string(mrb) }),
        };

        // Decode method name string. NULL pointer (not just an empty
        // string) is the wire violation — preserve the distinction by
        // checking the raw `mrb_str_to_cstr` pointer instead of going
        // through `to_string`, which collapses NULL into "".
        let method_ptr = unsafe { sys::mrb_str_to_cstr(mrb, method_val) };
        let method_name = if method_ptr.is_null() {
            // SAFETY: bridge frame — mruby unwinds through `mrb_raise`.
            unsafe { kobako.raise_wire_error(b"RPC method name is null\0") };
        } else {
            unsafe { core::ffi::CStr::from_ptr(method_ptr) }
                .to_str()
                .unwrap_or("")
        };

        // Decode positional args from the Array.
        let args_len = kobako.collection_len(args_ary);
        let mut wire_args = Vec::with_capacity(args_len);
        for i in 0..args_len {
            let elem = unsafe { sys::mrb_ary_entry(args_ary, i as i32) };
            wire_args.push(kobako.mrb_value_to_wire_value(elem));
        }

        // Decode kwargs from the Hash. Skip silently when kwargs_hash is
        // not actually a Hash (defensive — `oooo` unpack accepts any
        // object).
        let mut wire_kwargs = Vec::new();
        if unsafe { kwargs_hash.classname(mrb) } == "Hash" {
            kobako.decode_hash_kwargs(kwargs_hash, &mut wire_kwargs);
        }

        kobako.dispatch_invoke(
            target,
            method_name,
            &wire_args,
            &wire_kwargs,
            b"RPC wire error during invoke_rpc\0",
        )
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        sys::mrb_value::zeroed()
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
#[allow(unused_variables)]
pub(crate) unsafe extern "C" fn rpc_method_missing(
    mrb: *mut sys::mrb_state,
    self_: sys::mrb_value,
) -> sys::mrb_value {
    #[cfg(target_arch = "wasm32")]
    {
        use crate::envelope::Target;

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

/// `Kobako::Handle#initialize(id)` C bridge. Stores the Handle integer
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

/// `Kobako::Handle#method_missing(name, *args)` C bridge. Forwards every
/// method call on a Handle instance to the host via
/// `Kobako.__rpc_call__(id, method, args, kwargs)` with the Handle id
/// as an integer target (SPEC.md B-17 — Handle chaining).
#[allow(unused_variables)]
pub(crate) unsafe extern "C" fn handle_method_missing(
    mrb: *mut sys::mrb_state,
    self_: sys::mrb_value,
) -> sys::mrb_value {
    #[cfg(target_arch = "wasm32")]
    {
        use crate::envelope::Target;

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
        let _f1: sys::mrb_func_t = kobako_rpc_call;
        let _f2: sys::mrb_func_t = rpc_method_missing;
        let _f3: sys::mrb_func_t = rpc_respond_to_missing;
        let _f4: sys::mrb_func_t = handle_initialize;
        let _f5: sys::mrb_func_t = handle_method_missing;
        let _f6: sys::mrb_func_t = kernel_puts;
        let _f7: sys::mrb_func_t = kernel_p;
    }
}
