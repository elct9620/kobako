//! Implicit-receiver output delegators — `Kernel#print` / `#puts` /
//! `#printf` / `#p` / `#putc` / `#warn`, registered private on the
//! `Kernel` module and dispatching through the assignable `$stdout` /
//! `$stderr` globals at call time so guest scripts can rebind either
//! channel (docs/behavior.md B-04). The set mirrors mruby-io's
//! `mrblib/kernel.rb` write-path coverage; `warn` is a kobako
//! extension routed through `$stderr`.
//!
//! Registration goes through `sys::mrb_define_private_method` — the
//! delegators are private instance methods exactly as the previous
//! mrblib body declared them (mruby enforces visibility; a public
//! registration would let `42.puts("x")` dispatch). beni 0.1.0 has no
//! typed private-definition or global-read seam yet, so the bridges
//! here are hand-rolled `mrb_func_t` bodies over the `beni::sys`
//! escape hatch — `define_private_method` and `gv_get` are promotion
//! candidates for the wrapper.

use beni::sys;
use beni::{format, Mrb, Value};

/// Register the six private Kernel delegators.
pub(crate) fn install(mrb: &Mrb) {
    // SAFETY: `mrb` is live; `Kernel` is a core module present in
    // every state; each bridge below matches the `mrb_func_t` ABI and
    // the any-arity aspec mirrors the `(*)` mrblib signatures.
    unsafe {
        let kernel = sys::mrb_module_get(mrb.as_ptr(), c"Kernel".as_ptr());
        let any = sys::mrb_args_any_func();
        sys::mrb_define_private_method(mrb.as_ptr(), kernel, c"print".as_ptr(), kernel_print, any);
        sys::mrb_define_private_method(mrb.as_ptr(), kernel, c"puts".as_ptr(), kernel_puts, any);
        sys::mrb_define_private_method(
            mrb.as_ptr(),
            kernel,
            c"printf".as_ptr(),
            kernel_printf,
            any,
        );
        sys::mrb_define_private_method(mrb.as_ptr(), kernel, c"p".as_ptr(), kernel_p, any);
        sys::mrb_define_private_method(mrb.as_ptr(), kernel, c"putc".as_ptr(), kernel_putc, any);
        sys::mrb_define_private_method(mrb.as_ptr(), kernel, c"warn".as_ptr(), kernel_warn, any);
    }
}

/// Read a global variable (`$stdout` / `$stderr`) by name; an unset
/// variable reads as `nil`, and the subsequent funcall raises
/// `NoMethodError` exactly as the mrblib delegator would.
fn global(mrb: &Mrb, name: &core::ffi::CStr) -> Value {
    let sym = mrb.intern_cstr(name);
    // SAFETY: `mrb` is live; `mrb_gv_get` is a table read.
    Value::from_raw(unsafe { sys::mrb_gv_get(mrb.as_ptr(), sym) })
}

/// Shared body of the rest-args delegators: forward every positional
/// argument to `<global>.<method>` and return its result.
///
/// # Safety
///
/// `mrb` must be the live state mruby handed the calling bridge.
unsafe fn delegate_rest(
    mrb: *mut sys::mrb_state,
    target: &core::ffi::CStr,
    method: &core::ffi::CStr,
) -> sys::mrb_value {
    // SAFETY: mruby invokes bridges with a live state pointer that
    // outlives the call frame.
    let mrb = unsafe { Mrb::borrow_raw(&mrb) };
    // Copy out of the VM-stack arg window before the funcall can
    // reallocate it.
    let argv: Vec<Value> = mrb.get_args::<format::Rest>().to_vec();
    global(mrb, target).call(mrb, method, &argv).as_raw()
}

unsafe extern "C" fn kernel_print(
    mrb: *mut sys::mrb_state,
    _self: sys::mrb_value,
) -> sys::mrb_value {
    // SAFETY: bridge frame — see `delegate_rest`.
    unsafe { delegate_rest(mrb, c"$stdout", c"print") }
}

unsafe extern "C" fn kernel_puts(
    mrb: *mut sys::mrb_state,
    _self: sys::mrb_value,
) -> sys::mrb_value {
    // SAFETY: bridge frame — see `delegate_rest`.
    unsafe { delegate_rest(mrb, c"$stdout", c"puts") }
}

unsafe extern "C" fn kernel_printf(
    mrb: *mut sys::mrb_state,
    _self: sys::mrb_value,
) -> sys::mrb_value {
    // SAFETY: bridge frame — see `delegate_rest`.
    unsafe { delegate_rest(mrb, c"$stdout", c"printf") }
}

unsafe extern "C" fn kernel_p(mrb: *mut sys::mrb_state, _self: sys::mrb_value) -> sys::mrb_value {
    // SAFETY: bridge frame — see `delegate_rest`.
    unsafe { delegate_rest(mrb, c"$stdout", c"p") }
}

/// `Kernel#warn` routes through `$stderr.puts` — symmetric with the
/// `$stdout` delegators above.
unsafe extern "C" fn kernel_warn(
    mrb: *mut sys::mrb_state,
    _self: sys::mrb_value,
) -> sys::mrb_value {
    // SAFETY: bridge frame — see `delegate_rest`.
    unsafe { delegate_rest(mrb, c"$stderr", c"puts") }
}

/// `Kernel#putc` returns `nil`, not the argument — pinned by
/// mruby-io's `mrblib/kernel.rb`; the IO-level `IO#putc` does return
/// the original argument, and this delegator deliberately drops it.
unsafe extern "C" fn kernel_putc(
    mrb: *mut sys::mrb_state,
    _self: sys::mrb_value,
) -> sys::mrb_value {
    // SAFETY: mruby invokes bridges with a live state pointer that
    // outlives the call frame.
    let mrb = unsafe { Mrb::borrow_raw(&mrb) };
    let obj = mrb.get_args::<format::O>();
    global(mrb, c"$stdout").call(mrb, c"putc", &[obj]);
    Value::nil().as_raw()
}
