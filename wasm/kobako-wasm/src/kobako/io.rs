//! Top-level `::IO` class — a minimal write-only IO surface backing
//! `$stdout` / `$stderr` (and indirectly the `Kernel#puts` / `#print` /
//! `#warn` delegators in `mrblib/kernel.rb`).
//!
//! ## Why a separate file
//!
//! The IO surface is a separate concern from the Kobako module / RPC
//! handles housed in [`super::Kobako`]: there is no instance state to
//! cache beyond the fd ivar, and the bridges talk to wasi-libc's
//! `stdout` / `stderr` `FILE *` globals via a C shim
//! ([`crate::mruby::sys::kobako_io_fwrite`]) rather than re-entering
//! the Kobako token machinery.
//!
//! ## Shape vs. mruby-io
//!
//! Drop-in subset of `mrbgems/mruby-io`'s `IO` class: same constructor
//! signature (`IO.new(fd, mode)`), same instance methods on the
//! write-path (`#write`, `#fileno`). The Ruby-level methods (`#puts`,
//! `#print`, `#printf`, `#p`, `#<<`, `#tty?`, `#sync`, `#flush`,
//! `#closed?`) live in `mrblib/io.rb` and load via
//! [`crate::kobako::bytecode::load`] after this module registers the
//! C bridges.
//!
//! ## Scope restriction
//!
//! Only `fd == 1` (stdout) and `fd == 2` (stderr) are accepted at
//! construction. `mode` must be `"w"`. Anything else raises
//! `ArgumentError` immediately; the sandbox has no other captured fds
//! to route to.

use crate::mruby::sys;
use crate::mruby::sys::Value;

/// Install the top-level `::IO` class on `mrb` and load the
/// `mrblib/io.rb` instance-method surface. Idempotent (re-running this
/// against an already-installed state just re-defines the methods,
/// which is harmless given mruby's last-write-wins semantics).
/// wasm32-only — host callers do not run the IO install path.
///
/// # Safety
///
/// `mrb` must be a live mruby state. Intended to run inside
/// [`super::Kobako::install_raw`], which already holds the same
/// liveness contract.
#[cfg(target_arch = "wasm32")]
pub(crate) unsafe fn install(mrb: *mut sys::mrb_state) {
    // SAFETY: `mrb` is live per the function's safety contract.
    let mrb_ref = unsafe { crate::mruby::Mrb::borrow_raw(&mrb) };

    // Spell `Object` as the super class via the canonical
    // `mrb->object_class` field (mirrors `mrbgems/mruby-io/src/io.c`
    // line 2241). Passing a NULL super to `mrb_define_class` makes
    // mruby emit `"no super class for 'IO', Object assumed"` via
    // `mrb_warn` on every install, leaking onto the guest `stderr`
    // capture pipe (docs/behavior.md B-04).
    let io_class = mrb_ref.define_class(c"IO", mrb_ref.object_class());

    io_class.define_method(mrb_ref, c"initialize", io_initialize, sys::mrb_args_req(2));
    io_class.define_method(mrb_ref, c"write", io_write, sys::MRB_ARGS_ANY);
    io_class.define_method(mrb_ref, c"fileno", io_fileno, sys::MRB_ARGS_NONE);

    // Load the Ruby-level instance methods (#print / #puts / #printf
    // / #p / #<< / #tty? / #sync / #sync= / #flush / #closed?,
    // plus the `to_i` alias). The bytecode loader has the same
    // liveness contract as this function.
    unsafe {
        crate::kobako::bytecode::load(mrb, crate::kobako::bytecode::IO_MRB);
    }
}

/// `IO.new(fd, mode)` — initialize a sandbox-scoped IO bound to a
/// stdout / stderr file descriptor. Stores `fd` in `@__kobako_fd__`.
///
/// Raises `ArgumentError` when:
///   * `fd` is not 1 (stdout) or 2 (stderr) — the sandbox does not
///     route any other descriptor to the host capture pipe.
///   * `mode` is anything other than `"w"` — only the write-path is
///     implemented (mruby-io's read-path is intentionally out of
///     scope, see `mrblib/io.rb` class doc).
pub(crate) unsafe extern "C" fn io_initialize(mrb: *mut sys::mrb_state, self_: Value) -> Value {
    #[cfg(target_arch = "wasm32")]
    {
        // SAFETY: bridge frame — mruby invoked us with a live state.
        let mrb_ref = unsafe { crate::mruby::Mrb::borrow_raw(&mrb) };
        let (fd, mode_val) = mrb_ref.get_args_io();

        if fd != 1 && fd != 2 {
            unsafe {
                raise_argument_error(
                    mrb_ref,
                    c"kobako IO only supports fd 1 (stdout) or fd 2 (stderr)",
                );
            }
        }

        let mode = mode_val.to_string(mrb_ref);
        if mode != "w" {
            unsafe { raise_argument_error(mrb_ref, c"kobako IO only supports mode \"w\"") };
        }

        let fd_val = Value::from_int(mrb_ref, fd);
        let sym = mrb_ref.intern_cstr(c"@__kobako_fd__");
        self_.iv_set(mrb_ref, sym, fd_val);
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        // Host stub — see `io_write` for the shape rationale.
        let _ = mrb;
        let _ = self_;
    }
    Value::zeroed()
}

/// `IO#write(*objs)` — coerce each object via `Object#to_s` and pump
/// the bytes through `fwrite` to the descriptor-selected stream.
/// Returns the total bytes accepted (an `Integer`).
///
/// Truncation on cap exhaustion (docs/behavior.md B-04) surfaces as a short
/// return value: when wasmtime's `MemoryOutputPipe` rejects bytes past
/// its limit, `fwrite` short-writes and the returned total reflects
/// only the accepted bytes. No Ruby-level error is raised.
pub(crate) unsafe extern "C" fn io_write(mrb: *mut sys::mrb_state, self_: Value) -> Value {
    #[cfg(target_arch = "wasm32")]
    {
        // SAFETY: bridge frame — mruby invoked us with a live state.
        let mrb_ref = unsafe { crate::mruby::Mrb::borrow_raw(&mrb) };
        let fd = read_fd(mrb_ref, self_);
        let argv = mrb_ref.get_args_rest();

        // SAFETY: kobako_io_fwrite reads exactly `argv.len()` cells
        // from the argv buffer mruby produced on this call frame.
        // Value is `#[repr(transparent)]` over mrb_value so the slice
        // pointer reuses the layout.
        let total = unsafe {
            sys::kobako_io_fwrite(
                mrb,
                fd,
                argv.as_ptr() as *const sys::mrb_value,
                argv.len() as i32,
            )
        };
        Value::from_int(mrb_ref, total)
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

/// `IO#fileno` — returns the stored fd as an `Integer`. Used by the
/// `IO#to_i` alias in `mrblib/io.rb` and by introspecting callers
/// (e.g. `$stdout.fileno == 1`).
pub(crate) unsafe extern "C" fn io_fileno(mrb: *mut sys::mrb_state, self_: Value) -> Value {
    #[cfg(target_arch = "wasm32")]
    {
        // SAFETY: bridge frame — mruby invoked us with a live state.
        let mrb_ref = unsafe { crate::mruby::Mrb::borrow_raw(&mrb) };
        let fd = read_fd(mrb_ref, self_);
        Value::from_int(mrb_ref, fd)
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

/// Read the `@__kobako_fd__` ivar back to an `i32`. Returns 0 when the
/// ivar is missing or not Fixnum-tagged — neither case should arise in
/// practice because `io_initialize` is the only writer and stores the
/// fd as a boxed Integer. The downstream `kobako_io_fwrite` treats
/// `fd != 2` as "route to stdout", so a degenerate `0` lands on stdout
/// rather than trapping. The direct unbox skips the previous
/// `.to_s.parse` round-trip — see L-4 in the review for the rationale.
#[cfg(target_arch = "wasm32")]
fn read_fd(mrb: &crate::mruby::Mrb, self_: Value) -> i32 {
    let sym = mrb.intern_cstr(c"@__kobako_fd__");
    let val = self_.iv_get(mrb, sym);
    if !val.is_integer() {
        return 0;
    }
    // SAFETY: gated by the is_integer check above.
    unsafe { val.unbox_integer() }
}

/// Raise `ArgumentError` with `msg`. Diverges — `mrb_raise` does not
/// return.
///
/// # Safety
///
/// Only callable from contexts that mruby may unwind from (C bridges).
#[cfg(target_arch = "wasm32")]
unsafe fn raise_argument_error(mrb: &crate::mruby::Mrb, msg: &core::ffi::CStr) -> ! {
    let cls = mrb.class_get(c"ArgumentError");
    // SAFETY: bridge frame — caller upholds the unwind contract.
    unsafe { cls.raise(mrb, msg) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn c_bridges_have_mrb_func_t_signature() {
        // Compile-time signature check — mirrors the equivalent test
        // in `crate::kobako::bridges::tests`.
        let _f1: sys::mrb_func_t = io_initialize;
        let _f2: sys::mrb_func_t = io_write;
        let _f3: sys::mrb_func_t = io_fileno;
    }
}
