//! The `KobakoBridge` gem behind `super::Kobako::init`.
//!
//! Registers the Kobako class hierarchy + C bridges. The IO surface
//! is the sibling `kobako-io` crate's gem, composed alongside this
//! one by `super::Kobako::init`. `Mrb::init_gem` owns the panic
//! boundary around each `init`.

use beni::{Error, Gem, Module, Mrb, Object};

use super::bridges;

/// The Kobako module / class hierarchy and its C bridges — the unit
/// the published `kobako-mruby` crate ships as its one built-in gem.
/// `super::Kobako` re-resolves the registered class handles afterwards
/// via `resolve_raw`; `init` itself stays stateless per the `Gem`
/// contract.
pub(super) struct KobakoBridge;

impl Gem for KobakoBridge {
    /// Register the Kobako module, the `Kobako::Transport` namespace,
    /// the `Kobako::Transport::Proxy` abstract base plus its two
    /// top-level subclasses `Kobako::Member` and `Kobako::Handle`, and
    /// the `Kobako::ServiceError` / `Kobako::Transport::Error`
    /// exception hierarchy.
    ///
    /// Function pointers come from `bridges`, the only producer of
    /// `mrb_func_t` in this crate. Class handles produced by the
    /// definition calls are owned by mruby and live for the duration
    /// of `mrb`. An `Err` from any registration aborts the init and
    /// surfaces to the boot path as a Panic.
    fn init(mrb: &Mrb) -> Result<(), Error> {
        let object_class = mrb.object_class();

        // Kobako module.
        let kobako_mod = mrb.define_module(c"Kobako")?;

        // Kobako::Transport module — host↔guest message namespace shared
        // with the host gem's lib/kobako/transport.rb. Houses the Proxy
        // abstract base and the Error fault. The two proxy subclasses
        // `Kobako::Member` and `Kobako::Handle` live at the Kobako top level
        // — they are Sandbox-level domain entities (Member: bound-service
        // dispatch; Handle: service return / host-side argument
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
        // `Kobako::install_bindings`. Member calls arrive class-level (the
        // constant `MyService::KV` is a Member subclass), so `method_missing`
        // / `respond_to_missing?` are singleton-class methods routing to a
        // `Target::Path` derived from the class name. Subclasses inherit them
        // through the metaclass chain.
        let member_class = kobako_mod.define_class(mrb, c"Member", proxy_class)?;
        member_class.define_singleton_method(
            mrb,
            c"method_missing",
            beni::method!(bridges::member_method_missing, -1),
        )?;
        member_class.define_singleton_method(
            mrb,
            c"respond_to_missing?",
            beni::method!(bridges::proxy_respond_to_missing, -1),
        )?;
        // Block both construction entries so the guest cannot instantiate a
        // Member; see `bridges::member_not_constructible`.
        member_class.define_singleton_method(
            mrb,
            c"new",
            beni::method!(bridges::member_not_constructible, -1),
        )?;
        member_class.define_singleton_method(
            mrb,
            c"allocate",
            beni::method!(bridges::member_not_constructible, -1),
        )?;

        // `Kobako::Handle` — capability-handle proxy. Handle calls arrive
        // instance-level (a Handle is an instance carrying its id ivar), so
        // `method_missing` / `respond_to_missing?` are instance methods
        // routing to a `Target::Handle` derived from the id, and `initialize`
        // stores that id.
        let handle_class = kobako_mod.define_class(mrb, c"Handle", proxy_class)?;
        // Any-arity like the other bridge bodies: the body reads its one
        // argument through `format::O` itself (`FromValue` has no `Value`
        // identity impl to ride `method!`'s typed-parameter form), and the
        // only caller is the wire decoder's `mrb_obj_new`, which always
        // passes exactly the Handle id.
        handle_class.define_method(
            mrb,
            c"initialize",
            beni::method!(bridges::handle_initialize, -1),
        )?;
        // Block both construction entries so the guest cannot fabricate a
        // Handle from a bare id; see
        // `bridges::handle_not_constructible`. The wire decoder's restoration
        // path constructs Handles through `mrb_obj_new`, which bypasses these
        // Ruby entries and is unaffected.
        handle_class.define_singleton_method(
            mrb,
            c"new",
            beni::method!(bridges::handle_not_constructible, -1),
        )?;
        handle_class.define_singleton_method(
            mrb,
            c"allocate",
            beni::method!(bridges::handle_not_constructible, -1),
        )?;
        handle_class.define_method(
            mrb,
            c"method_missing",
            beni::method!(bridges::handle_method_missing, -1),
        )?;
        handle_class.define_method(
            mrb,
            c"respond_to_missing?",
            beni::method!(bridges::proxy_respond_to_missing, -1),
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
        // snippet-replay bytecode structural-failure path).
        kobako_mod.define_class(mrb, c"BytecodeError", runtime_error_class)?;

        Ok(())
    }
}
