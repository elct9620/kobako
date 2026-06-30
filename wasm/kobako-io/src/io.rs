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

use beni::{format, Error, FromValue, IntoValue, Mrb, RString, Value};

/// Install the IO surface on `mrb` — the top-level `::IO` class with
/// its full instance-method surface, then the `STDOUT` / `STDERR`
/// constants and the assignable `$stdout` / `$stderr` globals
/// constructed from it — the gem-init step named after mruby's own
/// `mrb_init_io`. The class is defined before the instances by
/// construction; the body order is the dependency order.
pub(crate) fn init(mrb: &Mrb) -> Result<(), beni::Error> {
    use beni::Module;

    // Spell `Object` as the super class via the canonical
    // `mrb->object_class` field (mirrors `mrbgems/mruby-io/src/io.c`).
    // Passing a NULL super to `mrb_define_class` makes mruby emit
    // `"no super class for 'IO', Object assumed"` via `mrb_warn` on
    // every install, leaking onto the guest `stderr` capture pipe.
    let io = mrb.define_class(c"IO", mrb.object_class())?;

    // `initialize` registers any-arity because its body reads the
    // call frame itself through `format::Io` — mruby's `"i"` integer
    // coercion for the fd has no typed-parameter equivalent. The
    // other multi-arg bodies read `format::Rest` / `format::O`
    // themselves for the same reason (`FromValue` has no `Value`
    // identity impl to ride `method!`'s typed-parameter form).
    io.define_method(mrb, c"initialize", beni::method!(io_initialize, -1))?;
    io.define_method(mrb, c"write", beni::method!(io_write, -1))?;
    io.define_method(mrb, c"fileno", beni::method!(io_fileno, 0))?;
    io.define_method(mrb, c"to_i", beni::method!(io_fileno, 0))?;
    io.define_method(mrb, c"print", beni::method!(io_print, -1))?;
    io.define_method(mrb, c"puts", beni::method!(io_puts, -1))?;
    io.define_method(mrb, c"printf", beni::method!(io_printf, -1))?;
    io.define_method(mrb, c"putc", beni::method!(io_putc, -1))?;
    io.define_method(mrb, c"p", beni::method!(io_p, -1))?;
    io.define_method(mrb, c"<<", beni::method!(io_lshift, -1))?;
    io.define_method(mrb, c"tty?", beni::method!(io_tty_p, 0))?;
    io.define_method(mrb, c"isatty", beni::method!(io_tty_p, 0))?;
    io.define_method(mrb, c"sync", beni::method!(io_sync, 0))?;
    io.define_method(mrb, c"sync=", beni::method!(io_sync_set, -1))?;
    io.define_method(mrb, c"flush", beni::method!(io_flush, 0))?;
    io.define_method(mrb, c"closed?", beni::method!(io_closed_p, 0))?;

    // Construct `STDOUT` / `STDERR` and wire `$stdout` / `$stderr` to
    // them. Guests can reassign either global at script time, which is
    // the whole point of routing through the Kernel delegators that
    // `crate::kernel_ext::init` registers afterwards.
    let mode_str = mrb.str_new_cstr(c"w").as_value();
    let stdout_val = io.obj_new(mrb, &[1i32.into_value(mrb), mode_str])?;
    let stderr_val = io.obj_new(mrb, &[2i32.into_value(mrb), mode_str])?;

    mrb.define_global_const(c"STDOUT", stdout_val);
    mrb.define_global_const(c"STDERR", stderr_val);

    mrb.gv_set(mrb.intern_cstr(c"$stdout"), stdout_val);
    mrb.gv_set(mrb.intern_cstr(c"$stderr"), stderr_val);
    Ok(())
}

/// `IO.new(fd, mode)` — initialize a sandbox-scoped IO bound to a
/// stdout / stderr file descriptor. Stores `fd` in `@__kobako_fd__`.
///
/// Raises `ArgumentError` when:
///   * `fd` is not 1 (stdout) or 2 (stderr) — the sandbox does not
///     route any other descriptor to the host capture pipe.
///   * `mode` is anything other than `"w"` — only the write-path is
///     implemented.
fn io_initialize(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let (fd, mode_val) = mrb.get_args::<format::Io>();

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

    // `fd` carries mruby's own `mrb_int` width, which follows the
    // target; `Value::from_int` takes it as-is on every width.
    let fd_val = Value::from_int(mrb, fd);
    let sym = mrb.intern_cstr(c"@__kobako_fd__");
    self_.iv_set(mrb, sym, fd_val)?;
    Ok(Value::zeroed())
}

/// `IO#write(*objs)` — coerce each object via `mrb_obj_as_string`
/// and pump the bytes through `write(2)` to the descriptor-selected
/// stream. Returns the total bytes accepted (an `Integer`).
///
/// Truncation on cap exhaustion surfaces as
/// a short return value: when wasmtime's `MemoryOutputPipe` rejects
/// bytes past its limit, `write(2)` short-writes and the returned
/// total reflects only the accepted bytes. No Ruby-level error is
/// raised.
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
    // Copy out of the VM-stack arg window before any funcall
    // (`obj_as_string` on a user type) can reallocate it.
    let argv: Vec<Value> = mrb.get_args::<format::Rest>().to_vec();

    let mut total: i32 = 0;
    for val in argv {
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

/// `IO#fileno` — returns the stored fd as an `Integer`. Also
/// registered as the `IO#to_i` alias.
fn io_fileno(mrb: &Mrb, self_: Value) -> Value {
    read_fd(mrb, self_).into_value(mrb)
}

/// `IO#print(*args)` — write each argument's `to_s` form, nothing
/// between or after. Returns `nil`.
fn io_print(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let argv: Vec<Value> = mrb.get_args::<format::Rest>().to_vec();
    for val in argv {
        let _scope = mrb.arena_scope();
        let s = val.obj_as_string(mrb)?;
        write_one(mrb, self_, s)?;
    }
    Ok(Value::nil())
}

/// `IO#puts(*args)` — newline-terminated write of each argument,
/// recursing into Arrays element-wise; no arguments writes a bare
/// newline. Returns `nil`.
fn io_puts(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let argv: Vec<Value> = mrb.get_args::<format::Rest>().to_vec();
    if argv.is_empty() {
        write_newline(mrb, self_)?;
        return Ok(Value::nil());
    }
    for val in argv {
        puts_one(mrb, self_, val)?;
    }
    Ok(Value::nil())
}

/// One `puts` element: Arrays recurse element-wise; anything else is
/// `to_s`-coerced, written, and newline-terminated unless the string
/// already ends with one.
fn puts_one(mrb: &Mrb, self_: Value, val: Value) -> Result<(), Error> {
    // Downcast on the value's type tag, not its classname: the tag
    // covers Array subclasses too, matching the `is_a?(Array)` check
    // the mrblib predecessor made.
    if let Some(ary) = beni::Array::from_value(val) {
        let len = collection_len(mrb, val)?;
        for i in 0..len {
            puts_one(mrb, self_, ary.entry(i as isize))?;
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

/// `IO#printf(format, *args)` — `sprintf` the arguments and write the
/// result. Returns `nil`.
///
/// `Kernel#sprintf` is reachable through funcall regardless of its
/// private visibility (`mrb_funcall_with_block` does not consult
/// `MRB_METHOD_PRIVATE_FL`) — the same implicit-self call the
/// previous mrblib body made.
fn io_printf(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let argv: Vec<Value> = mrb.get_args::<format::Rest>().to_vec();
    let formatted = self_.funcall(mrb, c"sprintf", &argv)?;
    write_one(mrb, self_, formatted)?;
    Ok(Value::nil())
}

/// `IO#putc(obj)` — mirrors mruby-io's `io_putc` (call-seq
/// `ios.putc(obj) -> obj`). Integer writes one byte (`obj & 0xff`);
/// String writes its first character (first byte in our non-UTF8
/// build); other objects coerce via `to_s`. Empty string is a no-op
/// write. Always returns the original argument.
fn io_putc(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let obj = mrb.get_args::<format::O>();
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

/// `IO#p(*args)` — write each argument's `inspect` form plus a
/// newline. Returns `nil` for no arguments, the argument itself for
/// one, and the argument Array for several — mirroring `Kernel#p`.
fn io_p(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let argv: Vec<Value> = mrb.get_args::<format::Rest>().to_vec();
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

/// `IO#<<(obj)` — write `obj` and return `self` for chaining.
fn io_lshift(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let obj = mrb.get_args::<format::O>();
    write_one(mrb, self_, obj)?;
    Ok(self_)
}

/// `IO#tty?` / `IO#isatty` — the sandbox pipes are never terminals.
fn io_tty_p(_mrb: &Mrb, _self: Value) -> Value {
    Value::false_()
}

/// `IO#sync` — reports whatever the guest last assigned via `#sync=`,
/// defaulting to `true` (the capture pipe is effectively unbuffered).
fn io_sync(mrb: &Mrb, self_: Value) -> Value {
    let sym = mrb.intern_cstr(c"@__kobako_sync");
    let v = self_.iv_get(mrb, sym);
    if v.is_nil() {
        Value::true_()
    } else {
        v
    }
}

/// `IO#sync=(value)` — store the flag; a no-op for the write path,
/// kept for mruby-io surface compatibility.
fn io_sync_set(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    let v = mrb.get_args::<format::O>();
    let sym = mrb.intern_cstr(c"@__kobako_sync");
    self_.iv_set(mrb, sym, v)?;
    Ok(v)
}

/// `IO#flush` — no-op (writes go straight to `write(2)`); returns
/// `self` for chaining.
fn io_flush(_mrb: &Mrb, self_: Value) -> Value {
    self_
}

/// `IO#closed?` — the sandbox streams cannot be closed.
fn io_closed_p(_mrb: &Mrb, _self: Value) -> Value {
    Value::false_()
}

/// Route one value through `self.write(...)` — the funcall keeps the
/// mrblib dispatch shape so a subclass overriding `#write` redirects
/// every composite method.
fn write_one(mrb: &Mrb, self_: Value, val: Value) -> Result<(), Error> {
    self_.funcall(mrb, c"write", &[val])?;
    Ok(())
}

/// Write a single `"\n"` through `self.write`.
fn write_newline(mrb: &Mrb, self_: Value) -> Result<(), Error> {
    let nl = mrb.str_new(b"\n").as_value();
    write_one(mrb, self_, nl)
}

/// Collection length via `.length`, mirroring the mruby core
/// implementation's Fixnum return; non-Fixnum (a user-overridden
/// `length` returning nonsense) reads as empty. A `length` that raises
/// surfaces as a guest exception so `puts` of a hostile Array
/// subclass attributes to the guest rather than corrupting the frame.
fn collection_len(mrb: &Mrb, col: Value) -> Result<usize, Error> {
    let len_val = col.funcall(mrb, c"length", &[])?;
    Ok(match i32::from_value(len_val) {
        Some(len) if len > 0 => len as usize,
        _ => 0,
    })
}

/// Build an `ArgumentError` carrying `msg`. A handler returns it as
/// `Err`, so the bridge frame raises it to the guest only after the
/// Rust frame has unwound — unlike a direct `mrb_raise` long-jump.
fn argument_error(mrb: &Mrb, msg: &str) -> Error {
    let cls = mrb
        .class_get(c"ArgumentError")
        .expect("ArgumentError is an mruby core class");
    Error::Exception(cls.exc_new(mrb, msg))
}

/// Read the `@__kobako_fd__` ivar back to an `i32`, or 0 when the ivar is
/// missing or not Fixnum-tagged. The value is untrusted: although
/// `io_initialize` only ever stores 1 or 2, the ivar is guest-mutable
/// (`instance_variable_set`), so any caller that forwards the result to a
/// syscall must re-validate the descriptor first — `io_write` does, refusing
/// anything outside {1, 2} before reaching `write(2)`.
fn read_fd(mrb: &Mrb, self_: Value) -> i32 {
    let sym = mrb.intern_cstr(c"@__kobako_fd__");
    let val = self_.iv_get(mrb, sym);
    i32::from_value(val).unwrap_or(0)
}
