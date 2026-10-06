//! Top-level `::IO` class — a minimal write-only IO surface backing
//! `$stdout` / `$stderr` (and indirectly the Kernel delegators in
//! `crate::kernel_ext`).
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
//! Only `fd == 1` (stdout) and `fd == 2` (stderr) are accepted at
//! construction. `mode` must be `"w"`. Anything else raises
//! `ArgumentError` immediately; the sandbox has no other captured fds
//! to route to.

use beni::prelude::*;
use beni::scan_args::scan_args;
use beni::{Array, Error, IntoValue, Mrb, RString, Value};

fn rest(mrb: &Mrb) -> Result<Vec<Value>, Error> {
    scan_args::<(), (), Array, (), (), ()>(mrb)?
        .splat
        .to_vec(mrb)
}

/// The gem-init step named after mruby's own `mrb_init_io`; the body order
/// is the dependency order, the class before the instances built from it.
pub(crate) fn init(mrb: &Mrb) -> Result<(), beni::Error> {
    use beni::Module;

    // Spell `Object` as the super class via the canonical
    // `mrb->object_class` field (mirrors `mrbgems/mruby-io/src/io.c`).
    // Passing a NULL super to `mrb_define_class` makes mruby emit
    // `"no super class for 'IO', Object assumed"` via `mrb_warn` on
    // every install, leaking onto the guest `stderr` capture pipe.
    let io = mrb.define_class(c"IO", mrb.object_class())?;

    // A body with a fixed argument list takes them as typed parameters;
    // only the variadic ones read the call frame themselves.
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
    io.define_method(mrb, c"closed?", beni::method!(io_closed_p, 0))?;

    // Construct `STDOUT` / `STDERR` and wire `$stdout` / `$stderr` to
    // them. Guests can reassign either global at script time, which is
    // the whole point of routing through the Kernel delegators that
    // `crate::kernel_ext::init` registers afterwards.
    let mode_str = mrb.str_new_cstr(c"w").as_value();
    let stdout_val = io.obj_new(mrb, &[1i32.into_value(mrb), mode_str])?;
    let stderr_val = io.obj_new(mrb, &[2i32.into_value(mrb), mode_str])?;

    mrb.define_global_const(c"STDOUT", stdout_val)?;
    mrb.define_global_const(c"STDERR", stderr_val)?;

    mrb.gv_set(c"$stdout", stdout_val)?;
    mrb.gv_set(c"$stderr", stderr_val)?;
    Ok(())
}

/// `IO.new(fd, mode)` refuses any `fd` but 1 or 2, since the sandbox
/// routes no other descriptor to the host capture pipe, and any `mode`
/// but `"w"`, since only the write path exists.
fn io_initialize(mrb: &Mrb, self_: Value, fd: i32, mode_val: Value) -> Result<Value, Error> {
    if fd != 1 && fd != 2 {
        return Err(argument_error(
            mrb,
            "kobako IO only supports fd 1 (stdout) or fd 2 (stderr)",
        ));
    }

    let mode = mode_val.to_string(mrb);
    if mode != "w" {
        return Err(argument_error(mrb, "kobako IO only supports mode \"w\""));
    }

    self_.iv_set(mrb, c"@__kobako_fd__", fd.into_value(mrb))?;
    Ok(Value::zeroed())
}

/// Truncation at the output cap surfaces as a short return value, not a
/// Ruby-level error: past the pipe's limit `write(2)` short-writes, and the
/// total counts only the accepted bytes.
fn io_write(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let fd = read_fd(mrb, self_);
    // The construction-time allowlist in `io_initialize` is not
    // self-enforcing: `@__kobako_fd__` is an ordinary ivar that guest mruby
    // can rewrite via `instance_variable_set`. Re-validate at the one place
    // the fd reaches a syscall, so the stdout / stderr restriction is an
    // enforced boundary rather than a construction-time courtesy.
    if fd != 1 && fd != 2 {
        return Err(argument_error(
            mrb,
            "kobako IO writes only to fd 1 (stdout) or fd 2 (stderr)",
        ));
    }
    // An owned copy, so the values outlive the `obj_as_string` funcalls
    // below — a borrow of the frame's own slots would not cross re-entry.
    let argv = rest(mrb)?;

    let mut total: i32 = 0;
    for &val in &argv {
        // A guest-defined `to_s` that raises propagates as an ordinary
        // guest exception instead of unwinding past this Rust frame.
        let s = val.obj_as_string(mrb)?;
        // SAFETY: `obj_as_string` returns a String-tagged Value;
        // the slice is consumed before the next mruby call.
        let bytes = unsafe { RString::from_value_unchecked(s).as_bytes(mrb) };
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

fn io_print(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let argv = rest(mrb)?;
    for &val in &argv {
        let _scope = mrb.arena_scope();
        let s = val.obj_as_string(mrb)?;
        write_one(mrb, self_, s)?;
    }
    Ok(Value::nil())
}

fn io_puts(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let argv = rest(mrb)?;
    if argv.is_empty() {
        write_newline(mrb, self_)?;
        return Ok(Value::nil());
    }
    for &val in &argv {
        puts_one(mrb, self_, val)?;
    }
    Ok(Value::nil())
}

fn puts_one(mrb: &Mrb, self_: Value, val: Value) -> Result<(), Error> {
    // Downcast on the value's type tag, not its classname: the tag
    // covers Array subclasses too, matching the `is_a?(Array)` check
    // the mrblib predecessor made.
    if let Some(ary) = beni::Array::from_value(val) {
        // Walk the C-level slots, never a Ruby `#each`: a hostile Array
        // subclass cannot override iteration to drive the recursion past the
        // real elements.
        for elem in ary.entries(mrb) {
            puts_one(mrb, self_, elem)?;
        }
        return Ok(());
    }
    let _scope = mrb.arena_scope();
    let s = val.obj_as_string(mrb)?;
    // SAFETY: `obj_as_string` returns a String-tagged Value; the
    // slice is dropped before the next mruby call below.
    let ends_nl = unsafe { RString::from_value_unchecked(s).as_bytes(mrb) }.last() == Some(&b'\n');
    write_one(mrb, self_, s)?;
    if !ends_nl {
        write_newline(mrb, self_)?;
    }
    Ok(())
}

/// `Kernel#sprintf` is reachable through funcall despite being private,
/// since `mrb_funcall_with_block` does not consult `MRB_METHOD_PRIVATE_FL`.
fn io_printf(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let argv = rest(mrb)?;
    let formatted = self_.funcall(mrb, c"sprintf", &argv)?;
    write_one(mrb, self_, formatted)?;
    Ok(Value::nil())
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
    let s = obj.obj_as_string(mrb)?;
    // SAFETY: `obj_as_string` returns a String-tagged Value; the
    // first byte is copied out before the next mruby call.
    let first = unsafe { RString::from_value_unchecked(s).as_bytes(mrb) }
        .first()
        .copied();
    if let Some(byte) = first {
        let one = mrb.str_new(&[byte]).as_value();
        write_one(mrb, self_, one)?;
    }
    Ok(obj)
}

/// The return value mirrors `Kernel#p`.
fn io_p(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let argv = rest(mrb)?;
    for &val in &argv {
        let _scope = mrb.arena_scope();
        let insp = val.funcall(mrb, c"inspect", &[])?;
        let nl = mrb.str_new(b"\n").as_value();
        self_.funcall(mrb, c"write", &[insp, nl])?;
    }
    Ok(match argv.len() {
        0 => Value::nil(),
        1 => argv[0],
        _ => {
            let ary = mrb.ary_new();
            for &val in &argv {
                ary.push(mrb, val)?;
            }
            ary.as_value()
        }
    })
}

fn io_lshift(mrb: &Mrb, self_: Value, obj: Value) -> Result<Value, Error> {
    write_one(mrb, self_, obj)?;
    Ok(self_)
}

/// The sandbox pipes are never terminals.
fn io_tty_p(_mrb: &Mrb, _self: Value) -> Value {
    Value::false_()
}

/// Defaults to `true`, since the capture pipe is effectively unbuffered.
fn io_sync(mrb: &Mrb, self_: Value) -> Value {
    let v = self_.iv_get(mrb, c"@__kobako_sync");
    if v.is_nil() {
        Value::true_()
    } else {
        v
    }
}

/// A no-op for the write path, kept for mruby-io surface compatibility.
fn io_sync_set(mrb: &Mrb, self_: Value, v: Value) -> Result<Value, Error> {
    self_.iv_set(mrb, c"@__kobako_sync", v)?;
    Ok(v)
}

/// A no-op, since writes go straight to `write(2)`.
fn io_flush(_mrb: &Mrb, self_: Value) -> Value {
    self_
}

/// The sandbox streams cannot be closed.
fn io_closed_p(_mrb: &Mrb, _self: Value) -> Value {
    Value::false_()
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
fn argument_error(mrb: &Mrb, msg: &str) -> Error {
    match mrb.exc_get(c"ArgumentError") {
        Ok(cls) => Error::new(mrb, cls, msg),
        Err(err) => err,
    }
}

/// The value is untrusted: the ivar is guest-mutable, so a caller that
/// forwards it to a syscall must re-validate the descriptor first.
fn read_fd(mrb: &Mrb, self_: Value) -> i32 {
    let val = self_.iv_get(mrb, c"@__kobako_fd__");
    i32::from_value(val).unwrap_or(0)
}
