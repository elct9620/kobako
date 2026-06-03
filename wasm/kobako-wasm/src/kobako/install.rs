//! Per-step install helpers for `super::Kobako::install`.
//!
//! `install` runs three independent registrations against a freshly
//! opened mruby state — class hierarchy, IO globals, Kernel delegators.
//! Keeping the steps in their own functions (rather than one ~150-line
//! body) lets +Kobako::install+ read as a four-line orchestration.
//!
//! The helpers are crate-private and wasm32-only by design — they
//! exist solely to support the wasm32 install path; the host target
//! never calls them because `super::Kobako` short-circuits to the
//! empty stub there.
//!
//! Keeps the façade (`super::Kobako`) lean by housing the bulk of the
//! boot wiring in sibling files like this one — the same
//! one-thing-per-file split the crate uses elsewhere.

use crate::mruby::sys;

#[cfg(target_arch = "wasm32")]
use super::bridges;
#[cfg(target_arch = "wasm32")]
use super::bytecode;
#[cfg(target_arch = "wasm32")]
use super::io;

/// Bundle of `crate::mruby::Class` handles produced by
/// `install_kobako_classes`. Internal to the install pipeline —
/// the caller pulls each handle into the matching field on
/// `super::Kobako`.
#[cfg(target_arch = "wasm32")]
pub(super) struct KobakoClasses {
    pub(super) member_class: crate::mruby::Class,
    pub(super) handle_class: crate::mruby::Class,
    pub(super) service_error_class: crate::mruby::Class,
    pub(super) transport_error_class: crate::mruby::Class,
}

/// Register the Kobako module, the `Kobako::Transport` namespace, the
/// `Kobako::Transport::Proxy` abstract base plus its two top-level
/// subclasses `Kobako::Member` and `Kobako::Handle`, and the
/// `Kobako::ServiceError` / `Kobako::Transport::Error` exception
/// hierarchy. Returns the class handles the `super::Kobako` token
/// needs to keep around.
///
/// Function pointers come from `bridges`, the only producer of
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
    // abstract base and the Error fault. The two proxy subclasses
    // `Kobako::Member` and `Kobako::Handle` live at the Kobako top level
    // — they are Sandbox-level domain entities (Member: bound-service
    // dispatch; Handle: B-14 service return / B-34 host-side argument
    // auto-wrap) and are not owned by the Transport namespace.
    let transport_mod = kobako_mod.define_module_under(mrb, c"Transport");

    // Kobako::Transport::Proxy — abstract base of `Kobako::Member` and
    // `Kobako::Handle`. It holds no dispatch methods itself; each
    // subclass registers its own `method_missing` for its receiver shape.
    // Spell the super class as `mrb.object_class()` to match the
    // mrbgems/mruby-io convention; passing NULL would log "no super class
    // for ..., Object assumed" via mrb_warn on every install.
    let proxy_class = transport_mod.define_class_under(mrb, c"Proxy", object_class);

    // `Kobako::Member` — base of every bound-Member proxy installed via
    // `Kobako::install_groups`. Member calls arrive class-level (the
    // constant `MyService::KV` is a Member subclass), so `method_missing`
    // / `respond_to_missing?` are singleton-class methods routing to a
    // `Target::Path` derived from the class name. Subclasses inherit them
    // through the metaclass chain.
    let member_class = kobako_mod.define_class_under(mrb, c"Member", proxy_class);
    member_class.define_singleton_method(
        mrb,
        c"method_missing",
        bridges::member_method_missing,
        sys::mrb_args_any(),
    );
    member_class.define_singleton_method(
        mrb,
        c"respond_to_missing?",
        bridges::proxy_respond_to_missing,
        sys::mrb_args_any(),
    );
    // Block both construction entries so the guest cannot instantiate a
    // Member (docs/behavior.md B-38); see `bridges::member_not_constructible`.
    member_class.define_singleton_method(
        mrb,
        c"new",
        bridges::member_not_constructible,
        sys::mrb_args_any(),
    );
    member_class.define_singleton_method(
        mrb,
        c"allocate",
        bridges::member_not_constructible,
        sys::mrb_args_any(),
    );

    // `Kobako::Handle` — capability-handle proxy. Handle calls arrive
    // instance-level (a Handle is an instance carrying its id ivar), so
    // `method_missing` / `respond_to_missing?` are instance methods
    // routing to a `Target::Handle` derived from the id, and `initialize`
    // stores that id.
    let handle_class = kobako_mod.define_class_under(mrb, c"Handle", proxy_class);
    handle_class.define_method(
        mrb,
        c"initialize",
        bridges::handle_initialize,
        sys::mrb_args_req(1),
    );
    // Block both construction entries so the guest cannot fabricate a
    // Handle from a bare id (docs/behavior.md B-39); see
    // `bridges::handle_not_constructible`. The wire decoder's restoration
    // path constructs Handles through `mrb_obj_new`, which bypasses these
    // Ruby entries and is unaffected.
    handle_class.define_singleton_method(
        mrb,
        c"new",
        bridges::handle_not_constructible,
        sys::mrb_args_any(),
    );
    handle_class.define_singleton_method(
        mrb,
        c"allocate",
        bridges::handle_not_constructible,
        sys::mrb_args_any(),
    );
    handle_class.define_method(
        mrb,
        c"method_missing",
        bridges::handle_method_missing,
        sys::mrb_args_any(),
    );
    handle_class.define_method(
        mrb,
        c"respond_to_missing?",
        bridges::proxy_respond_to_missing,
        sys::mrb_args_any(),
    );

    // `Kobako::ServiceError` / `Kobako::Transport::Error` /
    // `Kobako::BytecodeError` — all subclass `RuntimeError`.
    // ServiceError and BytecodeError stay at the Kobako top level
    // (public API); Error lives under Transport since it is a
    // transport-layer fault.
    let runtime_error_class = mrb.class_get(c"RuntimeError");
    let service_error_class =
        kobako_mod.define_class_under(mrb, c"ServiceError", runtime_error_class);
    let transport_error_class =
        transport_mod.define_class_under(mrb, c"Error", runtime_error_class);
    // `Kobako::BytecodeError` is registered here so guest code can
    // raise it by name; the class handle is not cached on
    // `KobakoClasses` because no compile-time-known call site reads
    // it yet — the snippet-replay path that uses it
    // ({docs/behavior.md E-37 / E-38}[link:../../../docs/behavior.md])
    // looks the class up lazily.
    kobako_mod.define_class_under(mrb, c"BytecodeError", runtime_error_class);

    KobakoClasses {
        member_class,
        handle_class,
        service_error_class,
        transport_error_class,
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
    // `#<<`, etc.) in pure Ruby. The `#write` bridge calls wasi-libc's
    // `write(2)` directly on the stored fd (1 = stdout, 2 = stderr)
    // (docs/behavior.md B-04).
    io::install(mrb);

    use crate::mruby::IntoValue;
    let io_class = mrb.class_get(c"IO");
    let mode_str = mrb.str_new_cstr(c"w");
    let stdout_val = io_class.obj_new(mrb, &[1i32.into_value(mrb), mode_str]);
    let stderr_val = io_class.obj_new(mrb, &[2i32.into_value(mrb), mode_str]);

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
/// `$stdout` / `$stderr`. Must run after `install_io_globals` —
/// the delegators look up the globals at call time but would
/// NoMethodError if called before they exist.
///
/// The bytecode blob is a `'static` `&[u8]` produced at build time
/// by mrbc.
#[cfg(target_arch = "wasm32")]
pub(super) fn install_kernel_delegators(mrb: &crate::mruby::Mrb) {
    bytecode::load(mrb, bytecode::KERNEL_MRB);
}
