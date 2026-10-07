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

use beni::prelude::*;
use beni::{Error, Mrb, Value};

/// The gem-init step named after mruby's own `mrb_init_kernel`.
pub(crate) fn init(mrb: &Mrb) -> Result<(), beni::Error> {
    let kernel = mrb.define_module(c"Kernel")?;
    kernel.define_private_method(mrb, c"print", beni::method!(kernel_print, -1))?;
    kernel.define_private_method(mrb, c"puts", beni::method!(kernel_puts, -1))?;
    kernel.define_private_method(mrb, c"printf", beni::method!(kernel_printf, -1))?;
    kernel.define_private_method(mrb, c"p", beni::method!(kernel_p, -1))?;
    kernel.define_private_method(mrb, c"putc", beni::method!(kernel_putc, 1))?;
    kernel.define_private_method(mrb, c"warn", beni::method!(kernel_warn, -1))?;
    Ok(())
}

/// An unset global reads as `nil`, so the funcall that follows raises
/// `NoMethodError` exactly as the mrblib delegator would.
fn global(mrb: &Mrb, name: &core::ffi::CStr) -> Value {
    mrb.gv_get(name)
}

fn kernel_print(mrb: &Mrb, _self: Value, args: &[Value]) -> Result<Value, Error> {
    global(mrb, c"$stdout").funcall(mrb, c"print", args)
}

fn kernel_puts(mrb: &Mrb, _self: Value, args: &[Value]) -> Result<Value, Error> {
    global(mrb, c"$stdout").funcall(mrb, c"puts", args)
}

fn kernel_printf(mrb: &Mrb, _self: Value, args: &[Value]) -> Result<Value, Error> {
    global(mrb, c"$stdout").funcall(mrb, c"printf", args)
}

fn kernel_p(mrb: &Mrb, _self: Value, args: &[Value]) -> Result<Value, Error> {
    global(mrb, c"$stdout").funcall(mrb, c"p", args)
}

/// `Kernel#warn` routes through `$stderr.puts` — symmetric with the
/// `$stdout` delegators above.
fn kernel_warn(mrb: &Mrb, _self: Value, args: &[Value]) -> Result<Value, Error> {
    global(mrb, c"$stderr").funcall(mrb, c"puts", args)
}

/// `Kernel#putc` returns `nil`, not the argument — pinned by
/// mruby-io's `mrblib/kernel.rb`; the IO-level `IO#putc` does return
/// the original argument, and this delegator deliberately drops it.
fn kernel_putc(mrb: &Mrb, _self: Value, obj: Value) -> Result<(), Error> {
    global(mrb, c"$stdout").funcall(mrb, c"putc", &[obj])?;
    Ok(())
}
