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
/// `super::Kobako` re-resolves the registered handles afterwards
/// via `resolve_raw`; `init` itself stays stateless per the `Gem`
/// contract.
pub(super) struct KobakoBridge;

impl Gem for KobakoBridge {
    /// Register the Kobako module, the `Kobako::Transport` namespace, the
    /// `Kobako::Proxy` capability module and the `Kobako::Handle` proxy
    /// that includes it, and the `Kobako::ServiceError` /
    /// `Kobako::Transport::Error` exception hierarchy.
    ///
    /// Function pointers come from `bridges`, the only producer of
    /// `mrb_func_t` in this crate. Handles produced by the definition
    /// calls are owned by mruby and live for the duration of `mrb`. An
    /// `Err` from any registration aborts the init and surfaces to the
    /// boot path as a Panic.
    fn init(mrb: &Mrb) -> Result<(), Error> {
        let object_class = mrb.object_class();

        // Kobako module.
        let kobako_mod = mrb.define_module(c"Kobako")?;

        // Kobako::Transport module — host↔guest message namespace shared
        // with the host gem's lib/kobako/transport.rb. Houses the Error
        // fault (a transport-layer wire violation).
        let transport_mod = kobako_mod.define_module(mrb, c"Transport")?;

        // Kobako::Proxy — the guest capability module that carries the
        // shared forwarding seam and nothing else. A bound-Service constant
        // extends it, so class-level calls forward with the constant's path
        // as `Target`; a `Kobako::Handle` includes it, so instance-level
        // calls forward with the instance's id. `method_missing` derives the
        // `Target` from the receiver's identity; `respond_to_missing?`
        // answers every probe optimistically.
        let proxy_module = kobako_mod.define_module(mrb, c"Proxy")?;
        proxy_module.define_method(
            mrb,
            c"method_missing",
            beni::method!(bridges::proxy_method_missing, -1),
        )?;
        proxy_module.define_method(
            mrb,
            c"respond_to_missing?",
            beni::method!(bridges::proxy_respond_to_missing, -1),
        )?;

        // `Kobako::Handle` — capability-handle proxy. Includes `Kobako::Proxy`
        // for instance-level forwarding (calls route to a `Target::Handle`
        // derived from the id `initialize` stores). Guest construction is
        // blocked at the class level so an exact `Kobako::Handle` arises only
        // from the wire decoder's `mrb_obj_new`; that keeps every
        // `Kobako::Handle` the guest sees host-issued, which is what
        // `proxy_method_missing`'s exact-identity check relies on.
        let handle_class = kobako_mod.define_class(mrb, c"Handle", object_class)?;
        handle_class.include_module(mrb, proxy_module)?;
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
        // Freeze a `dup`/`clone` copy so no duplication yields a re-pointable
        // Handle; see `bridges::handle_initialize_copy`.
        handle_class.define_method(
            mrb,
            c"initialize_copy",
            beni::method!(bridges::handle_initialize_copy, -1),
        )?;
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
