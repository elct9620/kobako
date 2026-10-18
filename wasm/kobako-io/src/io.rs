//! Top-level `::IO` class — the write surface backing `$stdout` /
//! `$stderr` (and indirectly the Kernel delegators in
//! `crate::kernel_ext`) and the files a guest opens through `File`.
//!
//! ## Shape vs. mruby-io
//!
//! Drop-in subset of `mrbgems/mruby-io`'s `IO` class: same constructor
//! signature (`IO.new(fd, mode)`), same write-path surface (`#write`,
//! `#fileno`, `#print`, `#puts`, `#printf`, `#putc`, `#p`, `#<<`,
//! `#tty?` / `#isatty`, `#sync` / `#sync=`, `#flush`, `#closed?`,
//! `#to_i`). The whole surface is registered as `beni` bridge methods
//! — the predecessor's `mrblib/io.rb` half is rewritten in Rust so
//! the gem ships no Ruby boot source and needs no mrbc pipeline.
//! Composite methods route their output through `self.write(...)`
//! funcalls, preserving the mrblib dispatch shape (a subclass
//! overriding `#write` redirects them all). Per-argument loops
//! bracket each iteration in `Mrb::arena_scope`: the mrblib
//! predecessors ran under the VM, which restores the GC arena every
//! instruction, so without the scope a long argument list would
//! accumulate arena slots in the C frame until overflow.
//!
//! ## Scope restriction
//!
//! Only `fd == 1` (stdout), `fd == 2` (stderr), and the files this guest
//! opened (`crate::descriptors`) are accepted at construction, and only
//! the String `"w"` as the `mode` of a captured one. A mode that is not a
//! String raises `TypeError`; any other fd or mode raises
//! `ArgumentError` immediately. Reading, seeking, and closing live in
//! `crate::stream`.

use beni::prelude::*;
use beni::value::{qnil, qtrue};
use beni::{Error, IntoValue, Mrb, RArray, RClass, RObject, RString, Value};
use core::ffi::CStr;

use crate::{descriptors, errno};

/// Where `IO.new` records the descriptor `write` later reads back.
pub(crate) const FD_IVAR: &CStr = c"@__kobako_fd__";

/// The gem-init step named after mruby's own `mrb_init_io`; the body order
/// is the dependency order, the class before the instances built from it.
pub(crate) fn init(mrb: &Mrb) -> Result<RClass, beni::Error> {
    use beni::Module;

    // Spell `Object` as the super class via the canonical
    // `mrb->object_class` field (mirrors `mrbgems/mruby-io/src/io.c`).
    // Passing a NULL super to `mrb_define_class` makes mruby emit
    // `"no super class for 'IO', Object assumed"` via `mrb_warn` on
    // every install, leaking onto the guest `stderr` capture pipe.
    let io = mrb.define_class(c"IO", mrb.object_class())?;

    // A body with a fixed argument list takes them as typed parameters;
    // the variadic ones take the call's arguments as a slice.
    io.define_method(mrb, c"initialize", beni::method!(io_initialize, 2))?;
    io.define_method(mrb, c"write", beni::method!(io_write, -1))?;
    io.define_method(mrb, c"fileno", beni::method!(io_fileno, 0))?;
    io.define_method(mrb, c"to_i", beni::method!(io_fileno, 0))?;
    io.define_method(mrb, c"print", beni::method!(io_print, -1))?;
    io.define_method(mrb, c"puts", beni::method!(io_puts, -1))?;
    io.define_method(mrb, c"printf", beni::method!(io_printf, -1))?;
    io.define_method(mrb, c"putc", beni::method!(io_putc, 1))?;
    io.define_method(mrb, c"p", beni::method!(io_p, -1))?;
    io.define_method(mrb, c"<<", beni::method!(io_lshift, 1))?;
    io.define_method(mrb, c"tty?", beni::method!(io_tty_p, 0))?;
    io.define_method(mrb, c"isatty", beni::method!(io_tty_p, 0))?;
    io.define_method(mrb, c"sync", beni::method!(io_sync, 0))?;
    io.define_method(mrb, c"sync=", beni::method!(io_sync_set, 1))?;
    io.define_method(mrb, c"flush", beni::method!(io_flush, 0))?;

    // Construct `STDOUT` / `STDERR` and wire `$stdout` / `$stderr` to
    // them. Guests can reassign either global at script time, which is
    // the whole point of routing through the Kernel delegators that
    // `crate::kernel_ext::init` registers afterwards.
    let mode_str = mrb.str_new_cstr(c"w").as_value();
    let stdout_val = io.new_instance(mrb, &[1i32.into_value(mrb), mode_str])?;
    let stderr_val = io.new_instance(mrb, &[2i32.into_value(mrb), mode_str])?;

    mrb.define_global_const(c"STDOUT", stdout_val)?;
    mrb.define_global_const(c"STDERR", stderr_val)?;

    mrb.gv_set(c"$stdout", stdout_val)?;
    mrb.gv_set(c"$stderr", stderr_val)?;
    Ok(io)
}

/// `IO.new(fd, mode)` refuses any `fd` but 1, 2, or a file this guest
/// opened: stdin carries the Preamble and a preopened directory is the
/// host's, so neither is the guest's to wrap. A captured descriptor only
/// writes, so its `mode` must be `"w"`.
fn io_initialize(mrb: &Mrb, self_: RObject, fd: i32, mode: RString) -> Result<(), Error> {
    if !descriptors::is_reachable(fd) {
        return Err(argument_error(
            mrb,
            "kobako IO only supports fd 1 (stdout), fd 2 (stderr), or a file this guest opened",
        ));
    }

    if descriptors::is_channel(fd) && String::from_value(mode.as_value()).as_deref() != Some("w") {
        return Err(argument_error(mrb, "kobako IO only supports mode \"w\""));
    }

    self_.ivar_set(mrb, FD_IVAR, fd)
}

/// Truncation at the output cap surfaces as a short return value, not a
/// Ruby-level error: past the pipe's limit `write(2)` short-writes, and the
/// total counts only the accepted bytes. A file's failed write raises the
/// `Errno` it names, as CRuby's does.
fn io_write(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<Value, Error> {
    // The construction-time allowlist in `io_initialize` is not
    // self-enforcing: `@__kobako_fd__` is an ordinary ivar that guest mruby
    // can rewrite via `instance_variable_set`. Re-validate where the fd
    // reaches a syscall, so the restriction holds rather than being a
    // construction-time courtesy.
    let fd = checked_fd(
        mrb,
        self_,
        "kobako IO writes only to fd 1 (stdout), fd 2 (stderr), or a file this guest opened",
    )?;
    let mut total: i32 = 0;
    for &val in args {
        let _scope = mrb.arena_scope();
        // A guest-defined `to_s` that raises propagates as an ordinary
        // guest exception instead of unwinding past this Rust frame.
        let s = val.to_r_string(mrb)?;
        // SAFETY: the slice is consumed before the next mruby call.
        let bytes = unsafe { s.as_bytes(mrb) };
        if !bytes.is_empty() {
            // SAFETY: ptr / len describe a live mruby-owned
            // buffer; `write(2)` reads it without retaining.
            let n = unsafe {
                write(
                    fd as core::ffi::c_int,
                    bytes.as_ptr() as *const core::ffi::c_void,
                    bytes.len(),
                )
            };
            if n < 0 && !descriptors::is_channel(fd) {
                let err = std::io::Error::last_os_error();
                return Err(errno::from_io(mrb, &err, &crate::stream::label(mrb, self_)));
            }
            if n > 0 {
                total = total.saturating_add(n as i32);
            }
        }
    }
    Ok(total.into_value(mrb))
}

unsafe extern "C" {
    /// wasi-libc `write(2)` syscall. Declared locally because this
    /// is a libc concern, not a mruby concern — keeping it out of
    /// the wrapper's surface preserves beni's mruby-only scope. The
    /// production target (wasm32-wasip1) auto-links wasi-libc; host
    /// targets resolve the same POSIX symbol from their libc.
    fn write(fd: core::ffi::c_int, buf: *const core::ffi::c_void, n: usize) -> isize;
}

fn io_fileno(mrb: &Mrb, self_: Value) -> Value {
    read_fd(mrb, self_).into_value(mrb)
}

fn io_print(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<(), Error> {
    for &val in args {
        let _scope = mrb.arena_scope();
        let s = val.to_r_string(mrb)?;
        write_one(mrb, self_, s.as_value())?;
    }
    Ok(())
}

fn io_puts(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<(), Error> {
    if args.is_empty() {
        return write_newline(mrb, self_);
    }
    for &val in args {
        puts_one(mrb, self_, val)?;
    }
    Ok(())
}

fn puts_one(mrb: &Mrb, self_: Value, val: Value) -> Result<(), Error> {
    // Downcast on the value's type tag, not its classname: the tag
    // covers Array subclasses too, matching the `is_a?(Array)` check
    // the mrblib predecessor made.
    if let Some(ary) = RArray::from_value(val) {
        // Walk the C-level slots, never a Ruby `#each`: a hostile Array
        // subclass cannot override iteration to drive the recursion past the
        // real elements.
        for elem in ary.entries(mrb) {
            puts_one(mrb, self_, elem)?;
        }
        return Ok(());
    }
    let _scope = mrb.arena_scope();
    let s = val.to_r_string(mrb)?;
    // SAFETY: the slice is dropped before the next mruby call below.
    let ends_nl = unsafe { s.as_bytes(mrb) }.last() == Some(&b'\n');
    write_one(mrb, self_, s.as_value())?;
    if !ends_nl {
        write_newline(mrb, self_)?;
    }
    Ok(())
}

/// `Kernel#sprintf` is reachable through funcall despite being private,
/// since `mrb_funcall_with_block` does not consult `MRB_METHOD_PRIVATE_FL`.
fn io_printf(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<(), Error> {
    let formatted = self_.funcall(mrb, c"sprintf", args)?;
    write_one(mrb, self_, formatted)
}

/// Mirrors mruby-io's `io_putc`; a String's first character is its first
/// byte in this non-UTF8 build.
fn io_putc(mrb: &Mrb, self_: Value, obj: Value) -> Result<Value, Error> {
    if let Some(n) = i32::from_value(obj) {
        let byte = [(n & 0xff) as u8];
        let s = mrb.str_new(&byte).as_value();
        write_one(mrb, self_, s)?;
        return Ok(obj);
    }
    let s = obj.to_r_string(mrb)?;
    // SAFETY: the first byte is copied out before the next mruby call.
    let first = unsafe { s.as_bytes(mrb) }.first().copied();
    if let Some(byte) = first {
        let one = mrb.str_new(&[byte]).as_value();
        write_one(mrb, self_, one)?;
    }
    Ok(obj)
}

/// The return value mirrors `Kernel#p`.
fn io_p(mrb: &Mrb, self_: Value, args: &[Value]) -> Result<Value, Error> {
    for &val in args {
        let _scope = mrb.arena_scope();
        let insp = val.funcall(mrb, c"inspect", &[])?;
        let nl = mrb.str_new(b"\n").as_value();
        self_.funcall(mrb, c"write", &[insp, nl])?;
    }
    Ok(match args {
        [] => qnil().as_value(),
        [only] => *only,
        _ => mrb.ary_new_from_values(args).as_value(),
    })
}

fn io_lshift(mrb: &Mrb, self_: Value, obj: Value) -> Result<Value, Error> {
    write_one(mrb, self_, obj)?;
    Ok(self_)
}

/// The sandbox pipes are never terminals.
fn io_tty_p(_mrb: &Mrb, _self: Value) -> bool {
    false
}

/// Defaults to `true`, since the capture pipe is effectively unbuffered.
fn io_sync(mrb: &Mrb, self_: RObject) -> Result<Value, Error> {
    let v: Value = self_.ivar_get(mrb, c"@__kobako_sync")?;
    Ok(if v.is_nil() { qtrue().as_value() } else { v })
}

/// A no-op for the write path, kept for mruby-io surface compatibility.
fn io_sync_set(mrb: &Mrb, self_: RObject, v: Value) -> Result<Value, Error> {
    self_.ivar_set(mrb, c"@__kobako_sync", v)?;
    Ok(v)
}

/// A no-op, since writes go straight to `write(2)`.
fn io_flush(_mrb: &Mrb, self_: Value) -> Value {
    self_
}

/// Dispatches through `self.write`, so a subclass overriding `#write`
/// redirects every composite method.
fn write_one(mrb: &Mrb, self_: Value, val: Value) -> Result<(), Error> {
    self_.funcall(mrb, c"write", &[val])?;
    Ok(())
}

fn write_newline(mrb: &Mrb, self_: Value) -> Result<(), Error> {
    let nl = mrb.str_new(b"\n").as_value();
    write_one(mrb, self_, nl)
}

/// Returned as `Err`, so the bridge frame raises it only after the Rust
/// frame has unwound — unlike a direct `mrb_raise` long-jump.
pub(crate) fn argument_error(mrb: &Mrb, msg: &str) -> Error {
    match mrb.exception_arg_error() {
        Ok(cls) => Error::new(mrb, cls, msg),
        Err(err) => err,
    }
}

/// The value is untrusted: the ivar is guest-mutable, so a caller that
/// forwards it to a syscall must re-validate the descriptor first.
fn read_fd(mrb: &Mrb, self_: Value) -> i32 {
    RObject::from_value(self_)
        .and_then(|obj| obj.ivar_get::<_, Value>(mrb, FD_IVAR).ok())
        .and_then(i32::from_value)
        .unwrap_or(0)
}

/// Whether `self_`'s descriptor was closed: `close` clears the ivar.
pub(crate) fn is_closed(mrb: &Mrb, self_: Value) -> bool {
    RObject::from_value(self_)
        .and_then(|obj| obj.ivar_get::<_, Value>(mrb, FD_IVAR).ok())
        .is_some_and(|fd| fd.is_nil())
}

/// The descriptor `self_` stands for, checked again where it reaches a
/// syscall: a closed stream is an `IOError`, and one rewritten to a
/// descriptor this guest may not reach is `refusal`.
pub(crate) fn checked_fd(mrb: &Mrb, self_: Value, refusal: &str) -> Result<i32, Error> {
    if is_closed(mrb, self_) {
        return Err(errno::io_error(mrb, "closed stream"));
    }
    let fd = read_fd(mrb, self_);
    if !descriptors::is_reachable(fd) {
        return Err(argument_error(mrb, refusal));
    }
    Ok(fd)
}
