//! The two `beni::Gem` units behind `super::Kobako::install`.
//!
//! `KobakoBridge` registers the class hierarchy + C bridges;
//! `KobakoIo` wires the IO globals and Kernel delegators. Splitting
//! along that line mirrors the planned guest-crate shape: the bridge
//! gem is the one built-in a future published `kobako` crate carries,
//! while the IO surface belongs to this shell (the mruby-io
//! precedent). `Mrb::init_gem` owns the panic boundary around each
//! `init`.
//!
//! The helpers are crate-private and wasm32-only by design — they
//! exist solely to support the wasm32 install path; the host target
//! never calls them because `super::Kobako` short-circuits to the
//! empty stub there.
//!
//! Keeps the façade (`super::Kobako`) lean by housing the bulk of the
//! boot wiring in sibling files like this one — the same
//! one-thing-per-file split the crate uses elsewhere.

#[cfg(target_arch = "wasm32")]
use crate::mruby::{Error, Gem, MethodDef, Module, Mrb, Object};

#[cfg(target_arch = "wasm32")]
use super::bridges;
#[cfg(target_arch = "wasm32")]
use super::bytecode;
#[cfg(target_arch = "wasm32")]
use super::io;

/// The Kobako module / class hierarchy and its C bridges — the unit a
/// future published `kobako` crate ships as its one built-in gem.
/// `super::Kobako` re-resolves the registered class handles afterwards
/// via `resolve_raw`; `init` itself stays stateless per the `Gem`
/// contract.
#[cfg(target_arch = "wasm32")]
pub(super) struct KobakoBridge;

#[cfg(target_arch = "wasm32")]
impl Gem for KobakoBridge {
    fn init(mrb: &Mrb) -> Result<(), Error> {
        install_kobako_classes(mrb)
    }
}

/// The sandbox IO surface — `::IO`, `STDOUT` / `STDERR`, `$stdout` /
/// `$stderr`, and the Kernel delegators. Shell-owned (not part of the
/// bridge gem), following the mruby-io precedent. Order inside `init`
/// matters: the delegators look up the globals at call time, so the
/// globals must be wired first.
#[cfg(target_arch = "wasm32")]
pub(super) struct KobakoIo;

#[cfg(target_arch = "wasm32")]
impl Gem for KobakoIo {
    fn init(mrb: &Mrb) -> Result<(), Error> {
        install_io_globals(mrb)?;
        install_kernel_delegators(mrb);
        Ok(())
    }
}

/// Register the Kobako module, the `Kobako::Transport` namespace, the
/// `Kobako::Transport::Proxy` abstract base plus its two top-level
/// subclasses `Kobako::Member` and `Kobako::Handle`, and the
/// `Kobako::ServiceError` / `Kobako::Transport::Error` exception
/// hierarchy.
///
/// Function pointers come from `bridges`, the only producer of
/// `mrb_func_t` in this crate. Class handles produced by the
/// definition calls are owned by mruby and live for the duration of
/// `mrb`. An `Err` from any registration aborts the install and
/// surfaces to the boot path as a Panic.
#[cfg(target_arch = "wasm32")]
fn install_kobako_classes(mrb: &Mrb) -> Result<(), Error> {
    let object_class = mrb.object_class();

    // Kobako module.
    let kobako_mod = mrb.define_module(c"Kobako")?;

    // Kobako::Transport module — host↔guest message namespace shared
    // with the host gem's lib/kobako/transport.rb. Houses the Proxy
    // abstract base and the Error fault. The two proxy subclasses
    // `Kobako::Member` and `Kobako::Handle` live at the Kobako top level
    // — they are Sandbox-level domain entities (Member: bound-service
    // dispatch; Handle: B-14 service return / B-34 host-side argument
    // auto-wrap) and are not owned by the Transport namespace.
    let transport_mod = kobako_mod.define_module(mrb, c"Transport")?;

    // Kobako::Transport::Proxy — abstract base of `Kobako::Member` and
    // `Kobako::Handle`. It holds no dispatch methods itself; each
    // subclass registers its own `method_missing` for its receiver shape.
    // Spell the super class as `mrb.object_class()` to match the
    // mrbgems/mruby-io convention; passing NULL would log "no super class
    // for ..., Object assumed" via mrb_warn on every install.
    let proxy_class = transport_mod.define_class(mrb, c"Proxy", object_class)?;

    // `Kobako::Member` — base of every bound-Member proxy installed via
    // `Kobako::install_groups`. Member calls arrive class-level (the
    // constant `MyService::KV` is a Member subclass), so `method_missing`
    // / `respond_to_missing?` are singleton-class methods routing to a
    // `Target::Path` derived from the class name. Subclasses inherit them
    // through the metaclass chain.
    let member_class = kobako_mod.define_class(mrb, c"Member", proxy_class)?;
    member_class.define_singleton_method(
        mrb,
        c"method_missing",
        MethodDef::new(bridges::member_method_missing, -1),
    )?;
    member_class.define_singleton_method(
        mrb,
        c"respond_to_missing?",
        MethodDef::new(bridges::proxy_respond_to_missing, -1),
    )?;
    // Block both construction entries so the guest cannot instantiate a
    // Member (docs/behavior.md B-38); see `bridges::member_not_constructible`.
    member_class.define_singleton_method(
        mrb,
        c"new",
        MethodDef::new(bridges::member_not_constructible, -1),
    )?;
    member_class.define_singleton_method(
        mrb,
        c"allocate",
        MethodDef::new(bridges::member_not_constructible, -1),
    )?;

    // `Kobako::Handle` — capability-handle proxy. Handle calls arrive
    // instance-level (a Handle is an instance carrying its id ivar), so
    // `method_missing` / `respond_to_missing?` are instance methods
    // routing to a `Target::Handle` derived from the id, and `initialize`
    // stores that id.
    let handle_class = kobako_mod.define_class(mrb, c"Handle", proxy_class)?;
    handle_class.define_method(
        mrb,
        c"initialize",
        MethodDef::new(bridges::handle_initialize, 1),
    )?;
    // Block both construction entries so the guest cannot fabricate a
    // Handle from a bare id (docs/behavior.md B-39); see
    // `bridges::handle_not_constructible`. The wire decoder's restoration
    // path constructs Handles through `mrb_obj_new`, which bypasses these
    // Ruby entries and is unaffected.
    handle_class.define_singleton_method(
        mrb,
        c"new",
        MethodDef::new(bridges::handle_not_constructible, -1),
    )?;
    handle_class.define_singleton_method(
        mrb,
        c"allocate",
        MethodDef::new(bridges::handle_not_constructible, -1),
    )?;
    handle_class.define_method(
        mrb,
        c"method_missing",
        MethodDef::new(bridges::handle_method_missing, -1),
    )?;
    handle_class.define_method(
        mrb,
        c"respond_to_missing?",
        MethodDef::new(bridges::proxy_respond_to_missing, -1),
    )?;

    // `Kobako::ServiceError` / `Kobako::Transport::Error` /
    // `Kobako::BytecodeError` — all subclass `RuntimeError`.
    // ServiceError and BytecodeError stay at the Kobako top level
    // (public API); Error lives under Transport since it is a
    // transport-layer fault.
    let runtime_error_class = mrb.class_get(c"RuntimeError")?;
    kobako_mod.define_class(mrb, c"ServiceError", runtime_error_class)?;
    transport_mod.define_class(mrb, c"Error", runtime_error_class)?;
    // `Kobako::BytecodeError` is registered here so guest code can
    // raise it by name; like every handle this gem registers, call
    // sites re-resolve it lazily (`super::Kobako::resolve_raw`, the
    // snippet-replay path of
    // {docs/behavior.md E-37 / E-38}[link:../../../docs/behavior.md]).
    kobako_mod.define_class(mrb, c"BytecodeError", runtime_error_class)?;

    Ok(())
}

/// Register the top-level `::IO` class (constructor + `#write` /
/// `#fileno` C bridges and the `mrblib/io.rb` instance-method surface)
/// then construct `STDOUT` / `STDERR` and wire `$stdout` / `$stderr`
/// to them. Guests can reassign either global at script time, which
/// is the whole point of routing through the kernel delegators that
/// load next.
#[cfg(target_arch = "wasm32")]
fn install_io_globals(mrb: &Mrb) -> Result<(), Error> {
    // Top-level `::IO` class. Registers the constructor + `#write` /
    // `#fileno` C bridges and then loads `mrblib/io.rb` to layer the
    // rest of the IO surface (`#print`, `#puts`, `#printf`, `#p`,
    // `#<<`, etc.) in pure Ruby. The `#write` bridge calls wasi-libc's
    // `write(2)` directly on the stored fd (1 = stdout, 2 = stderr)
    // (docs/behavior.md B-04).
    io::install(mrb)?;

    use crate::mruby::IntoValue;
    let io_class = mrb.class_get(c"IO")?;
    let mode_str = mrb.str_new_cstr(c"w");
    let stdout_val = io_class.obj_new(mrb, &[1i32.into_value(mrb), mode_str]);
    let stderr_val = io_class.obj_new(mrb, &[2i32.into_value(mrb), mode_str]);

    mrb.define_global_const(c"STDOUT", stdout_val);
    mrb.define_global_const(c"STDERR", stderr_val);

    let stdout_gvar = mrb.intern_cstr(c"$stdout");
    let stderr_gvar = mrb.intern_cstr(c"$stderr");
    mrb.gv_set(stdout_gvar, stdout_val);
    mrb.gv_set(stderr_gvar, stderr_val);
    Ok(())
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
fn install_kernel_delegators(mrb: &Mrb) {
    bytecode::load(mrb, bytecode::KERNEL_MRB);
}
