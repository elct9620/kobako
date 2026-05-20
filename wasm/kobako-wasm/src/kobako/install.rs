//! Per-step install helpers for [`super::Kobako::install_raw`].
//!
//! `install_raw` runs three independent registrations against a
//! freshly opened mruby state — class hierarchy, IO globals, Kernel
//! delegators — each with its own preconditions. Keeping the steps in
//! their own functions (rather than one ~150-line `unsafe { ... }`
//! body) lets each carry a focused SAFETY note and lets `install_raw`
//! read as a four-line orchestration.
//!
//! The helpers are crate-private and wasm32-only by design — they
//! exist solely to support the wasm32 install path; the host target
//! never calls them because [`super::Kobako`] short-circuits to the
//! empty stub there.
//!
//! Mirrors the host-side install split in `lib/kobako/registry/` —
//! `install` here plays the same role as the per-service-group
//! `service_group.rb` modules: the façade ([`super::Kobako`]) stays
//! lean while the bulk of the boot wiring lives in sibling files.

use crate::mruby::sys;
#[cfg(target_arch = "wasm32")]
use crate::mruby::value::cstr_ptr;

#[cfg(target_arch = "wasm32")]
use super::bridges;
#[cfg(target_arch = "wasm32")]
use super::bytecode;
#[cfg(target_arch = "wasm32")]
use super::io;

#[cfg(target_arch = "wasm32")]
use super::names::*;

/// Bundle of `RClass *` handles produced by
/// [`install_kobako_classes`]. Internal to the install pipeline —
/// the caller pulls each handle into the matching field on
/// [`super::Kobako`].
#[cfg(target_arch = "wasm32")]
pub(super) struct KobakoClasses {
    pub(super) client_class: *mut sys::RClass,
    pub(super) handle_class: *mut sys::RClass,
    pub(super) service_error_class: *mut sys::RClass,
    pub(super) disconnected_class: *mut sys::RClass,
    pub(super) wire_error_class: *mut sys::RClass,
}

/// Register the Kobako module, the `Kobako::RPC` namespace, the
/// `Kobako::RPC::Client` / `Kobako::RPC::Handle` classes, and the
/// `Kobako::ServiceError` / `Disconnected` / `Kobako::RPC::WireError`
/// exception hierarchy. Returns the five class handles the
/// [`super::Kobako`] token needs to keep around.
///
/// # Safety
///
/// `mrb` must be a live mruby state. Every C-string passed
/// (`cstr_ptr(*_NAME)`) is compile-time NUL-terminated. Function
/// pointers come from [`bridges`], the only producer of
/// `mrb_func_t` in this crate. Class handles returned by
/// `mrb_define_module` / `mrb_define_class_under` are owned by
/// mruby and live for the duration of `mrb`.
#[cfg(target_arch = "wasm32")]
pub(super) unsafe fn install_kobako_classes(mrb: *mut sys::mrb_state) -> KobakoClasses {
    // SAFETY: see item-level doc.
    unsafe {
        // Kobako module.
        let kobako_mod = sys::mrb_define_module(mrb, cstr_ptr(KOBAKO_NAME));

        // Kobako::RPC module — protocol namespace shared with the
        // host gem's lib/kobako/rpc.rb. Houses the Client base
        // class plus Handle / WireError value objects that ride on
        // the wire.
        let rpc_mod = sys::mrb_define_module_under(mrb, kobako_mod, cstr_ptr(RPC_NAME));

        // Kobako::RPC::Client base class — parent of every Member
        // installed via `Kobako::install_groups`. Spell the super
        // class as `(*mrb).object_class` to match the
        // mrbgems/mruby-io convention; passing NULL would log
        // "no super class for ..., Object assumed" via mrb_warn on
        // every install.
        let client_class =
            sys::mrb_define_class_under(mrb, rpc_mod, cstr_ptr(CLIENT_NAME), (*mrb).object_class);

        // Singleton-class `method_missing` / `respond_to_missing?`
        // on `Kobako::RPC::Client`. Subclasses inherit through the
        // metaclass-chain dispatch.
        sys::mrb_define_singleton_method(
            mrb,
            client_class as *mut sys::RObject,
            cstr_ptr(METHOD_MISSING_NAME),
            bridges::rpc_method_missing,
            sys::MRB_ARGS_ANY,
        );
        sys::mrb_define_singleton_method(
            mrb,
            client_class as *mut sys::RObject,
            cstr_ptr(RESPOND_TO_MISSING_NAME),
            bridges::rpc_respond_to_missing,
            sys::MRB_ARGS_ANY,
        );

        // `Kobako::RPC::Handle` instance class. Same explicit
        // `(*mrb).object_class` super as the Client class above.
        let handle_class =
            sys::mrb_define_class_under(mrb, rpc_mod, cstr_ptr(HANDLE_NAME), (*mrb).object_class);
        sys::mrb_define_method(
            mrb,
            handle_class,
            cstr_ptr(INITIALIZE_NAME),
            bridges::handle_initialize,
            sys::mrb_args_req(1),
        );
        sys::mrb_define_method(
            mrb,
            handle_class,
            cstr_ptr(METHOD_MISSING_NAME),
            bridges::handle_method_missing,
            sys::MRB_ARGS_ANY,
        );
        sys::mrb_define_method(
            mrb,
            handle_class,
            cstr_ptr(RESPOND_TO_MISSING_NAME),
            bridges::rpc_respond_to_missing,
            sys::MRB_ARGS_ANY,
        );

        // `Kobako::ServiceError` / `Kobako::ServiceError::Disconnected`
        // / `Kobako::RPC::WireError` / `Kobako::BytecodeError` — all
        // subclass `RuntimeError`. ServiceError and BytecodeError stay
        // at the Kobako top level (public API); WireError lives under
        // RPC since it is an RPC-layer fault.
        let runtime_error_class = sys::mrb_class_get(mrb, cstr_ptr(RUNTIME_ERROR_NAME));
        let service_error_class = sys::mrb_define_class_under(
            mrb,
            kobako_mod,
            cstr_ptr(SERVICE_ERROR_NAME),
            runtime_error_class,
        );
        let disconnected_class = sys::mrb_define_class_under(
            mrb,
            service_error_class,
            cstr_ptr(DISCONNECTED_NAME),
            service_error_class,
        );
        let wire_error_class = sys::mrb_define_class_under(
            mrb,
            rpc_mod,
            cstr_ptr(WIRE_ERROR_NAME),
            runtime_error_class,
        );
        // `Kobako::BytecodeError` is registered here so guest code can
        // raise it by name; the class handle is not cached on
        // `KobakoClasses` because no compile-time-known call site reads
        // it yet — the snippet-replay path that uses it
        // ({docs/behavior.md E-37 / E-38}[link:../../../docs/behavior.md])
        // looks the class up lazily.
        sys::mrb_define_class_under(
            mrb,
            kobako_mod,
            cstr_ptr(BYTECODE_ERROR_NAME),
            runtime_error_class,
        );

        KobakoClasses {
            client_class,
            handle_class,
            service_error_class,
            disconnected_class,
            wire_error_class,
        }
    }
}

/// Register the top-level `::IO` class (constructor + `#write` /
/// `#fileno` C bridges and the `mrblib/io.rb` instance-method surface)
/// then construct `STDOUT` / `STDERR` and wire `$stdout` / `$stderr`
/// to them. Guests can reassign either global at script time, which
/// is the whole point of routing through the kernel delegators that
/// load next.
///
/// # Safety
///
/// As [`install_kobako_classes`].
#[cfg(target_arch = "wasm32")]
pub(super) unsafe fn install_io_globals(mrb: *mut sys::mrb_state) {
    // SAFETY: see item-level doc.
    unsafe {
        // Top-level `::IO` class. Registers the constructor + `#write`
        // / `#fileno` C bridges and then loads `mrblib/io.rb` to layer
        // the rest of the IO surface (`#print`, `#puts`, `#printf`,
        // `#p`, `#<<`, etc.) in pure Ruby. The bridges talk to
        // wasi-libc's `stdout` / `stderr` via the `kobako_io_fwrite`
        // C shim (docs/behavior.md B-04).
        io::install(mrb);

        let io_class = sys::mrb_class_get(mrb, cstr_ptr(IO_NAME));
        let mode_str = sys::mrb_str_new_cstr(mrb, cstr_ptr(MODE_WRITE));
        let stdout_args = [sys::mrb_boxing_int_value(mrb, 1), mode_str];
        let stdout_val = sys::mrb_obj_new(mrb, io_class, 2, stdout_args.as_ptr());
        let stderr_args = [sys::mrb_boxing_int_value(mrb, 2), mode_str];
        let stderr_val = sys::mrb_obj_new(mrb, io_class, 2, stderr_args.as_ptr());

        sys::mrb_define_global_const(mrb, cstr_ptr(STDOUT_CONST_NAME), stdout_val);
        sys::mrb_define_global_const(mrb, cstr_ptr(STDERR_CONST_NAME), stderr_val);

        let stdout_gvar = sys::mrb_intern_cstr(mrb, cstr_ptr(STDOUT_GVAR_NAME));
        let stderr_gvar = sys::mrb_intern_cstr(mrb, cstr_ptr(STDERR_GVAR_NAME));
        sys::mrb_gv_set(mrb, stdout_gvar, stdout_val);
        sys::mrb_gv_set(mrb, stderr_gvar, stderr_val);
    }
}

/// Load the precompiled `mrblib/kernel.rb` bytecode. The blob
/// redefines `Kernel#print` (overriding mruby-core's `mrb_print_m`
/// registration that always targets the C `stdout` FILE*) and adds
/// `#puts` / `#p` / `#printf` / `#warn` as thin pass-throughs to
/// `$stdout` / `$stderr`. Must run after [`install_io_globals`] —
/// the delegators look up the globals at call time but would
/// NoMethodError if called before they exist.
///
/// # Safety
///
/// As [`install_kobako_classes`]. The bytecode blob is a `'static`
/// `&[u8]` produced at build time by mrbc.
#[cfg(target_arch = "wasm32")]
pub(super) unsafe fn install_kernel_delegators(mrb: *mut sys::mrb_state) {
    // SAFETY: see item-level doc.
    unsafe {
        bytecode::load(mrb, bytecode::KERNEL_MRB);
    }
}
