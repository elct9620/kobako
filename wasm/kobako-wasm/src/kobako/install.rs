//! Per-step install helpers for [`super::Kobako::install`].
//!
//! `install` runs three independent registrations against a freshly
//! opened mruby state — class hierarchy, IO globals, Kernel delegators.
//! Keeping the steps in their own functions (rather than one ~150-line
//! body) lets +Kobako::install+ read as a four-line orchestration.
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
use super::bridges;
#[cfg(target_arch = "wasm32")]
use super::bytecode;
#[cfg(target_arch = "wasm32")]
use super::io;

/// Bundle of [`sys::Class`] handles produced by
/// [`install_kobako_classes`]. Internal to the install pipeline —
/// the caller pulls each handle into the matching field on
/// [`super::Kobako`].
#[cfg(target_arch = "wasm32")]
pub(super) struct KobakoClasses {
    pub(super) proxy_class: sys::Class,
    pub(super) handle_class: sys::Class,
    pub(super) service_error_class: sys::Class,
    pub(super) disconnected_class: sys::Class,
    pub(super) wire_error_class: sys::Class,
}

/// Register the Kobako module, the `Kobako::Transport` namespace, the
/// `Kobako::Transport::Proxy` class plus the top-level `Kobako::Handle`
/// value object, and the `Kobako::ServiceError` / `Disconnected` /
/// `Kobako::Transport::WireError` exception hierarchy. Returns the
/// five class handles the [`super::Kobako`] token needs to keep around.
///
/// Function pointers come from [`bridges`], the only producer of
/// `mrb_func_t` in this crate. Class handles produced by
/// `define_*_under` are owned by mruby and live for the duration of
/// `mrb`.
#[cfg(target_arch = "wasm32")]
pub(super) fn install_kobako_classes(mrb: &crate::mruby::Mrb) -> KobakoClasses {
    let object_class = mrb.object_class();

    // Kobako module.
    let kobako_mod = mrb.define_module(c"Kobako");

    // Kobako::Transport module — host↔guest message namespace shared
    // with the host gem's lib/kobako/transport.rb. Houses the Proxy
    // base class (parent of every Member proxy installed via
    // `Kobako::install_groups`) and the WireError fault. The Handle
    // value object lives at top level (Kobako::Handle) — it is a
    // Sandbox-level domain entity used in both directions across the
    // host↔guest boundary (B-14 service return, B-34 host-side
    // argument auto-wrap) and is not owned by the Transport namespace.
    let transport_mod = kobako_mod.define_module_under(mrb, c"Transport");

    // Kobako::Transport::Proxy base class — parent of every Member
    // installed via `Kobako::install_groups`. Spell the super class as
    // `mrb.object_class()` to match the mrbgems/mruby-io convention;
    // passing NULL would log "no super class for ..., Object assumed"
    // via mrb_warn on every install.
    let proxy_class = transport_mod.define_class_under(mrb, c"Proxy", object_class);

    // Singleton-class `method_missing` / `respond_to_missing?` on
    // `Kobako::Transport::Proxy`. Subclasses inherit through the
    // metaclass-chain dispatch.
    proxy_class.define_singleton_method(
        mrb,
        c"method_missing",
        bridges::transport_proxy_method_missing,
        sys::MRB_ARGS_ANY,
    );
    proxy_class.define_singleton_method(
        mrb,
        c"respond_to_missing?",
        bridges::transport_proxy_respond_to_missing,
        sys::MRB_ARGS_ANY,
    );

    // `Kobako::Handle` instance class — top-level Sandbox-level value
    // object, not nested under Transport. Same explicit
    // `mrb.object_class()` super as the Proxy class above.
    //
    // The Handle bridges share +forward_to_dispatch+ with the Proxy
    // bridge above (see bridges.rs); collapsing the two bridges into a
    // single class-shape-aware shim is deferred until Phase 3 — the
    // TODO lives next to +handle_method_missing+.
    let handle_class = kobako_mod.define_class_under(mrb, c"Handle", object_class);
    handle_class.define_method(
        mrb,
        c"initialize",
        bridges::handle_initialize,
        sys::mrb_args_req(1),
    );
    handle_class.define_method(
        mrb,
        c"method_missing",
        bridges::handle_method_missing,
        sys::MRB_ARGS_ANY,
    );
    handle_class.define_method(
        mrb,
        c"respond_to_missing?",
        bridges::transport_proxy_respond_to_missing,
        sys::MRB_ARGS_ANY,
    );

    // `Kobako::ServiceError` / `Kobako::ServiceError::Disconnected`
    // / `Kobako::Transport::WireError` / `Kobako::BytecodeError` — all
    // subclass `RuntimeError`. ServiceError and BytecodeError stay
    // at the Kobako top level (public API); WireError lives under
    // Transport since it is a transport-layer fault.
    let runtime_error_class = mrb.class_get(c"RuntimeError");
    let service_error_class =
        kobako_mod.define_class_under(mrb, c"ServiceError", runtime_error_class);
    let disconnected_class =
        service_error_class.define_class_under(mrb, c"Disconnected", service_error_class);
    let wire_error_class = transport_mod.define_class_under(mrb, c"WireError", runtime_error_class);
    // `Kobako::BytecodeError` is registered here so guest code can
    // raise it by name; the class handle is not cached on
    // `KobakoClasses` because no compile-time-known call site reads
    // it yet — the snippet-replay path that uses it
    // ({docs/behavior.md E-37 / E-38}[link:../../../docs/behavior.md])
    // looks the class up lazily.
    kobako_mod.define_class_under(mrb, c"BytecodeError", runtime_error_class);

    KobakoClasses {
        proxy_class,
        handle_class,
        service_error_class,
        disconnected_class,
        wire_error_class,
    }
}

/// Register the top-level `::IO` class (constructor + `#write` /
/// `#fileno` C bridges and the `mrblib/io.rb` instance-method surface)
/// then construct `STDOUT` / `STDERR` and wire `$stdout` / `$stderr`
/// to them. Guests can reassign either global at script time, which
/// is the whole point of routing through the kernel delegators that
/// load next.
#[cfg(target_arch = "wasm32")]
pub(super) fn install_io_globals(mrb: &crate::mruby::Mrb) {
    // Top-level `::IO` class. Registers the constructor + `#write` /
    // `#fileno` C bridges and then loads `mrblib/io.rb` to layer the
    // rest of the IO surface (`#print`, `#puts`, `#printf`, `#p`,
    // `#<<`, etc.) in pure Ruby. The bridges talk to wasi-libc's
    // `stdout` / `stderr` via the `kobako_io_fwrite` C shim
    // (docs/behavior.md B-04).
    io::install(mrb);

    let io_class = mrb.class_get(c"IO");
    let mode_str = mrb.str_new_cstr(c"w");
    let stdout_val = io_class.obj_new(mrb, &[sys::Value::from_int(mrb, 1), mode_str]);
    let stderr_val = io_class.obj_new(mrb, &[sys::Value::from_int(mrb, 2), mode_str]);

    mrb.define_global_const(c"STDOUT", stdout_val);
    mrb.define_global_const(c"STDERR", stderr_val);

    let stdout_gvar = mrb.intern_cstr(c"$stdout");
    let stderr_gvar = mrb.intern_cstr(c"$stderr");
    mrb.gv_set(stdout_gvar, stdout_val);
    mrb.gv_set(stderr_gvar, stderr_val);
}

/// Load the precompiled `mrblib/kernel.rb` bytecode. The blob
/// redefines `Kernel#print` (overriding mruby-core's `mrb_print_m`
/// registration that always targets the C `stdout` FILE*) and adds
/// `#puts` / `#p` / `#printf` / `#warn` as thin pass-throughs to
/// `$stdout` / `$stderr`. Must run after [`install_io_globals`] —
/// the delegators look up the globals at call time but would
/// NoMethodError if called before they exist.
///
/// The bytecode blob is a `'static` `&[u8]` produced at build time
/// by mrbc.
#[cfg(target_arch = "wasm32")]
pub(super) fn install_kernel_delegators(mrb: &crate::mruby::Mrb) {
    bytecode::load(mrb, bytecode::KERNEL_MRB);
}
