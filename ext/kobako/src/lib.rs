//! The Ruby gem's native half. Each Context's module registers the Ruby
//! constants it answers for; the helpers every module shares sit beside
//! them.

use magnus::{prelude::*, Error, RModule, RString, Ruby, Symbol, TryConvert, Value};

mod errors;
mod escape;
mod filesystem;
mod gvl;
mod network;
mod pinned_value;
mod runtime;

#[magnus::init]
fn init(ruby: &Ruby) -> Result<(), Error> {
    let kobako = ruby.define_module("Kobako")?;
    // This extension is the only frontend in its process, so the first
    // registration is the one that stands.
    let _ = kobako_runtime::sched::register_blocking(Box::new(gvl::GvlFreeWait));
    runtime::register(ruby, kobako)?;
    filesystem::register(ruby, kobako)?;
    network::register(kobako)?;
    Ok(())
}

/// The one place the boundary borrows an RString's bytes: the borrow does
/// not outlive this call, so no Ruby allocation can move the RString
/// between the borrow and the copy.
fn to_bytes(s: RString) -> Vec<u8> {
    // SAFETY: see item doc.
    unsafe { s.as_slice() }.to_vec()
}

/// The constant `path` names beneath `Kobako`. `lib/kobako` defines each
/// one the ext reads before the ext reads it, so a missing one is a wiring
/// bug and the `unwrap` fails fast.
fn constant_by_path<T: TryConvert>(ruby: &Ruby, path: &[&str]) -> T {
    let kobako: RModule = ruby.class_object().const_get("Kobako").unwrap();
    let (name, modules) = path.split_last().expect("a path names a constant");
    let owner = modules
        .iter()
        .fold(kobako, |owner, module| owner.const_get(*module).unwrap());
    owner.const_get(*name).unwrap()
}

/// The name of `value` when it is a Symbol, so each choice an option or a
/// field names is read the same way.
fn symbol_name_of(value: Value) -> Result<Option<String>, Error> {
    Symbol::from_value(value)
        .map(|symbol| symbol.name().map(|name| name.into_owned()))
        .transpose()
}
