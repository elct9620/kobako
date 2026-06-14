//! Implicit-receiver output delegators — `Kernel#print` / `#puts` /
//! `#printf` / `#p` / `#putc` / `#warn`, registered private on the
//! `Kernel` module and dispatching through the assignable `$stdout` /
//! `$stderr` globals at call time so guest scripts can rebind either
//! channel. The set mirrors mruby-io's
//! `mrblib/kernel.rb` write-path coverage; `warn` is a kobako
//! extension routed through `$stderr`.
//!
//! The delegators register private exactly as the previous mrblib
//! body declared them — mruby enforces visibility, so a public
//! registration would let `42.puts("x")` dispatch. Bodies are safe
//! `method!` delegators; `Mrb::define_module` returns the existing
//! core module, so the `Kernel` lookup is the same idempotent call
//! every gem uses.

use beni::{format, Error, Module, Mrb, Value};

/// Register the six private Kernel delegators — the gem-init step
/// named after mruby's own `mrb_init_kernel`.
pub(crate) fn init(mrb: &Mrb) -> Result<(), beni::Error> {
    let kernel = mrb.define_module(c"Kernel")?;
    kernel.define_private_method(mrb, c"print", beni::method!(kernel_print, -1))?;
    kernel.define_private_method(mrb, c"puts", beni::method!(kernel_puts, -1))?;
    kernel.define_private_method(mrb, c"printf", beni::method!(kernel_printf, -1))?;
    kernel.define_private_method(mrb, c"p", beni::method!(kernel_p, -1))?;
    kernel.define_private_method(mrb, c"putc", beni::method!(kernel_putc, -1))?;
    kernel.define_private_method(mrb, c"warn", beni::method!(kernel_warn, -1))?;
    Ok(())
}

/// Read a global variable (`$stdout` / `$stderr`) by name; an unset
/// variable reads as `nil`, and the subsequent funcall raises
/// `NoMethodError` exactly as the mrblib delegator would.
fn global(mrb: &Mrb, name: &core::ffi::CStr) -> Value {
    mrb.gv_get(mrb.intern_cstr(name))
}

/// Shared body of the rest-args delegators: forward every positional
/// argument to `<global>.<method>` and return its result.
fn delegate_rest(
    mrb: &Mrb,
    target: &core::ffi::CStr,
    method: &core::ffi::CStr,
) -> Result<Value, Error> {
    // Copy out of the VM-stack arg window before the funcall can
    // reallocate it.
    let argv: Vec<Value> = mrb.get_args::<format::Rest>().to_vec();
    global(mrb, target).funcall(mrb, method, &argv)
}

fn kernel_print(mrb: &Mrb, _self: Value) -> Result<Value, Error> {
    delegate_rest(mrb, c"$stdout", c"print")
}

fn kernel_puts(mrb: &Mrb, _self: Value) -> Result<Value, Error> {
    delegate_rest(mrb, c"$stdout", c"puts")
}

fn kernel_printf(mrb: &Mrb, _self: Value) -> Result<Value, Error> {
    delegate_rest(mrb, c"$stdout", c"printf")
}

fn kernel_p(mrb: &Mrb, _self: Value) -> Result<Value, Error> {
    delegate_rest(mrb, c"$stdout", c"p")
}

/// `Kernel#warn` routes through `$stderr.puts` — symmetric with the
/// `$stdout` delegators above.
fn kernel_warn(mrb: &Mrb, _self: Value) -> Result<Value, Error> {
    delegate_rest(mrb, c"$stderr", c"puts")
}

/// `Kernel#putc` returns `nil`, not the argument — pinned by
/// mruby-io's `mrblib/kernel.rb`; the IO-level `IO#putc` does return
/// the original argument, and this delegator deliberately drops it.
fn kernel_putc(mrb: &Mrb, _self: Value) -> Result<Value, Error> {
    let obj = mrb.get_args::<format::O>();
    global(mrb, c"$stdout").funcall(mrb, c"putc", &[obj])?;
    Ok(Value::nil())
}
