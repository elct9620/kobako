//! Implicit-receiver output — `Kernel#print` / `#puts` / `#printf` / `#p` /
//! `#putc` / `#warn`, registered private on the `Kernel` module and reaching
//! the assignable `$stdout` / `$stderr` globals at call time, so a script can
//! rebind either channel to any object answering `write`, as CRuby's do.
//!
//! The writers register private, so `42.puts("x")` does not dispatch.
//! `Mrb::define_module` returns the existing core module, so the `Kernel`
//! lookup is the same idempotent call every gem uses.

use beni::prelude::*;
use beni::value::qnil;
use beni::{Error, Mrb, RHash, RString, Value};

use crate::io::{p_to, print_to, printf_to, puts_lines};

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

/// An unset global reads as `nil`, so the call that follows raises
/// `NoMethodError`.
fn global(mrb: &Mrb, name: &core::ffi::CStr) -> Result<Value, Error> {
    mrb.gv_get(name)
}

/// Each argument written to `$stdout` through its `write`, as `rb_f_print`.
fn kernel_print(mrb: &Mrb, _self: Value, args: &[Value]) -> Result<Value, Error> {
    print_to(mrb, global(mrb, c"$stdout")?, args)?;
    Ok(qnil().as_value())
}

/// Handed to `$stdout.puts`, as `rb_f_puts` forwards to a rebound `$stdout`.
fn kernel_puts(mrb: &Mrb, _self: Value, args: &[Value]) -> Result<Value, Error> {
    global(mrb, c"$stdout")?.funcall(mrb, c"puts", args)
}

/// Nothing for no arguments; a leading String formats to `$stdout`, and
/// anything else leading is where the rest is written, as `rb_f_printf`.
fn kernel_printf(mrb: &Mrb, _self: Value, args: &[Value]) -> Result<Value, Error> {
    match args {
        [] => {}
        [format, ..] if RString::from_value(*format).is_some() => {
            printf_to(mrb, global(mrb, c"$stdout")?, args)?;
        }
        [out, rest @ ..] => printf_to(mrb, *out, rest)?,
    }
    Ok(qnil().as_value())
}

fn kernel_p(mrb: &Mrb, _self: Value, args: &[Value]) -> Result<Value, Error> {
    p_to(mrb, global(mrb, c"$stdout")?, args)
}

/// Answers `obj`, as `Kernel#putc` answers what `IO#putc` does.
fn kernel_putc(mrb: &Mrb, _self: Value, obj: Value) -> Result<Value, Error> {
    global(mrb, c"$stdout")?.funcall(mrb, c"putc", [obj])
}

/// The messages laid out as `puts` lays them out, written to `$stderr` in one
/// `write`, as `rb_warn_m` hands `Warning.warn` one String; nothing for no
/// message. `uplevel:` and `category:` are taken and read as nothing, since
/// the guest names no caller's location and filters no category.
fn kernel_warn(mrb: &Mrb, _self: Value, args: &[Value]) -> Result<Value, Error> {
    let messages = match args.split_last() {
        Some((last, rest)) if RHash::from_value(*last).is_some() => rest,
        _ => args,
    };
    if messages.is_empty() {
        return Ok(qnil().as_value());
    }
    let mut text = Vec::new();
    puts_lines(mrb, messages, &mut |line| {
        let line = line.to_r_string(mrb)?;
        // SAFETY: the bytes are copied out before the next mruby call.
        text.extend_from_slice(unsafe { line.as_slice(mrb) });
        Ok(())
    })?;
    let text = mrb.str_new(&text).as_value();
    global(mrb, c"$stderr")?.funcall::<_, _, Value>(mrb, c"write", [text])?;
    Ok(qnil().as_value())
}
