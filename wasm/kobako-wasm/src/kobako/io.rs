//! Top-level `::IO` class — a minimal write-only IO surface backing
//! `$stdout` / `$stderr` (and indirectly the `Kernel#puts` / `#print` /
//! `#warn` delegators in `mrblib/kernel.rb`).
//!
//! ## Why a separate file
//!
//! The IO surface is its own concern from the Kobako module / RPC
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

#[cfg(target_arch = "wasm32")]
use crate::cstr;
use crate::mruby::sys;
#[cfg(target_arch = "wasm32")]
use crate::mruby::value::cstr_ptr;

#[cfg(target_arch = "wasm32")]
const IO_NAME: &[u8] = b"IO\0";
#[cfg(target_arch = "wasm32")]
const INITIALIZE_NAME: &[u8] = b"initialize\0";
#[cfg(target_arch = "wasm32")]
const WRITE_NAME: &[u8] = b"write\0";
#[cfg(target_arch = "wasm32")]
const FILENO_NAME: &[u8] = b"fileno\0";
/// `b"@__kobako_fd__\0"` — mangled ivar that holds the underlying file
/// descriptor (1 or 2) as a boxed Integer. The bridges round-trip
/// through `to_string + parse` to stay consistent with the existing
/// handle-id ivar pattern in [`super::Kobako::extract_handle_id`].
#[cfg(target_arch = "wasm32")]
const FD_IVAR: &[u8] = b"@__kobako_fd__\0";
#[cfg(target_arch = "wasm32")]
const ARGUMENT_ERROR_NAME: &[u8] = b"ArgumentError\0";

/// Install the top-level `::IO` class on `mrb` and load the
/// `mrblib/io.rb` instance-method surface. Idempotent (re-running this
/// against an already-installed state just re-defines the methods,
/// which is harmless given mruby's last-write-wins semantics).
///
/// # Safety
///
/// `mrb` must be a live mruby state. Intended to run inside
/// [`super::Kobako::install_raw`], which already holds the same
/// liveness contract.
#[cfg_attr(not(target_arch = "wasm32"), allow(unused_variables))]
pub unsafe fn install(mrb: *mut sys::mrb_state) {
    #[cfg(target_arch = "wasm32")]
    {
        // SAFETY: `mrb` is live per the function's safety contract.
        // Every C-string passed (`cstr_ptr(*_NAME)`) is NUL-terminated.
        // The function-pointer arguments are `unsafe extern "C" fn`
        // items from this module — the only producer of `mrb_func_t`
        // for the IO class in this crate.
        unsafe {
            let io_class = sys::mrb_define_class(mrb, cstr_ptr(IO_NAME), core::ptr::null_mut());

            sys::mrb_define_method(
                mrb,
                io_class,
                cstr_ptr(INITIALIZE_NAME),
                io_initialize,
                sys::mrb_args_req(2),
            );
            sys::mrb_define_method(
                mrb,
                io_class,
                cstr_ptr(WRITE_NAME),
                io_write,
                sys::MRB_ARGS_ANY,
            );
            sys::mrb_define_method(
                mrb,
                io_class,
                cstr_ptr(FILENO_NAME),
                io_fileno,
                sys::MRB_ARGS_NONE,
            );
        }

        // Load the Ruby-level instance methods (#print / #puts / #printf
        // / #p / #<< / #tty? / #sync / #sync= / #flush / #closed?,
        // plus the `to_i` alias). The bytecode loader has the same
        // liveness contract as this function.
        unsafe {
            crate::kobako::bytecode::load(mrb, crate::kobako::bytecode::IO_MRB);
        }
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
#[allow(unused_variables)]
pub(crate) unsafe extern "C" fn io_initialize(
    mrb: *mut sys::mrb_state,
    self_: sys::mrb_value,
) -> sys::mrb_value {
    #[cfg(target_arch = "wasm32")]
    {
        let mut fd: core::ffi::c_int = 0;
        let mut mode_val = sys::mrb_value::zeroed();
        unsafe {
            sys::mrb_get_args(
                mrb,
                cstr!("io"),
                &mut fd as *mut core::ffi::c_int,
                &mut mode_val as *mut sys::mrb_value,
            );
        }

        if fd != 1 && fd != 2 {
            unsafe {
                raise_argument_error(
                    mrb,
                    b"kobako IO only supports fd 1 (stdout) or fd 2 (stderr)\0",
                );
            }
        }

        let mode = unsafe { mode_val.to_string(mrb) };
        if mode != "w" {
            unsafe {
                raise_argument_error(mrb, b"kobako IO only supports mode \"w\"\0");
            }
        }

        let fd_val = unsafe { sys::mrb_boxing_int_value(mrb, fd) };
        unsafe {
            let sym = sys::mrb_intern_cstr(mrb, cstr_ptr(FD_IVAR));
            sys::mrb_iv_set(mrb, self_, sym, fd_val);
        }
    }
    sys::mrb_value::zeroed()
}

/// `IO#write(*objs)` — coerce each object via `Object#to_s` and pump
/// the bytes through `fwrite` to the descriptor-selected stream.
/// Returns the total bytes accepted (an `Integer`).
///
/// Truncation on cap exhaustion (SPEC.md B-04) surfaces as a short
/// return value: when wasmtime's `MemoryOutputPipe` rejects bytes past
/// its limit, `fwrite` short-writes and the returned total reflects
/// only the accepted bytes. No Ruby-level error is raised.
#[allow(unused_variables)]
pub(crate) unsafe extern "C" fn io_write(
    mrb: *mut sys::mrb_state,
    self_: sys::mrb_value,
) -> sys::mrb_value {
    #[cfg(target_arch = "wasm32")]
    {
        let fd = unsafe { read_fd(mrb, self_) };

        let mut argv: *const sys::mrb_value = core::ptr::null();
        let mut argc: core::ffi::c_int = 0;
        unsafe {
            sys::mrb_get_args(
                mrb,
                cstr!("*"),
                &mut argv as *mut *const sys::mrb_value,
                &mut argc as *mut core::ffi::c_int,
            );
        }

        let total = unsafe { sys::kobako_io_fwrite(mrb, fd, argv, argc) };
        unsafe { sys::mrb_boxing_int_value(mrb, total) }
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        sys::mrb_value::zeroed()
    }
}

/// `IO#fileno` — returns the stored fd as an `Integer`. Used by the
/// `IO#to_i` alias in `mrblib/io.rb` and by introspecting callers
/// (e.g. `$stdout.fileno == 1`).
#[allow(unused_variables)]
pub(crate) unsafe extern "C" fn io_fileno(
    mrb: *mut sys::mrb_state,
    self_: sys::mrb_value,
) -> sys::mrb_value {
    #[cfg(target_arch = "wasm32")]
    {
        let fd = unsafe { read_fd(mrb, self_) };
        unsafe { sys::mrb_boxing_int_value(mrb, fd) }
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        sys::mrb_value::zeroed()
    }
}

/// Read the `@__kobako_fd__` ivar back to an `i32`. Returns 0 when the
/// ivar is missing or non-numeric — neither case should arise in
/// practice because `io_initialize` is the only writer and rejects
/// invalid fds. The downstream `kobako_io_fwrite` treats `fd != 2` as
/// "route to stdout", so a degenerate `0` lands on stdout rather than
/// trapping.
#[cfg(target_arch = "wasm32")]
unsafe fn read_fd(mrb: *mut sys::mrb_state, self_: sys::mrb_value) -> i32 {
    unsafe {
        let sym = sys::mrb_intern_cstr(mrb, cstr_ptr(FD_IVAR));
        let val = sys::mrb_iv_get(mrb, self_, sym);
        val.to_string(mrb).parse().unwrap_or(0)
    }
}

/// Raise `ArgumentError` with a NUL-terminated message. Diverges —
/// `mrb_raise` does not return.
///
/// # Safety
///
/// Only callable from contexts that mruby may unwind from (C bridges).
/// `msg` must be NUL-terminated.
#[cfg(target_arch = "wasm32")]
unsafe fn raise_argument_error(mrb: *mut sys::mrb_state, msg: &[u8]) -> ! {
    unsafe {
        let cls = sys::mrb_class_get(mrb, cstr_ptr(ARGUMENT_ERROR_NAME));
        sys::mrb_raise(mrb, cls, msg.as_ptr() as *const core::ffi::c_char);
    }
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
