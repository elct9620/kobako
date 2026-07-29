//! `MyService::Session` — a capability Handle a script calls methods on.
//!
//! The host issues a Handle as an id; what a script holds it as is this
//! gem's decision. A class of its own, with real methods, keeps the
//! whole surface fixed: `session.get(key)` reaches the host under the
//! same schema `MyService::KV.get` does, and only the envelope's target
//! differs — an id rather than a path.
//!
//! Instances arise only from the wire. `new` and `allocate` raise, and
//! `mint` builds one through the C-level constructor those two do not
//! guard, so a script cannot name an id it was never handed. The host is
//! the boundary either way — an unissued id is refused there, and the
//! table lives for one invocation — but a Handle a script can forge
//! stops meaning what it says, and this is what it costs to keep it
//! meaning that.

use beni::{Error, Mrb, RClass, RModule, RString, Value};
use kobako_mruby::Target;

use crate::dispatch::{self, runtime_error, type_error};

/// Where an instance keeps the id it stands for.
const HANDLE_IVAR: &core::ffi::CStr = c"@__kv_handle__";

/// Define `MyService::Session` under `service`.
pub(crate) fn init(mrb: &Mrb, service: RModule) -> Result<(), Error> {
    use beni::{Module, Object};

    let session = service.define_class(mrb, c"Session", mrb.object_class())?;
    session.define_method(mrb, c"initialize", beni::method!(initialize, 1))?;
    // A `dup` would otherwise yield an unfrozen copy, and an unfrozen
    // copy is re-pointable at an id it was never given.
    session.define_method(mrb, c"initialize_copy", beni::method!(initialize_copy, -1))?;
    session.define_singleton_method(mrb, c"new", beni::method!(not_constructible, -1))?;
    session.define_singleton_method(mrb, c"allocate", beni::method!(not_constructible, -1))?;
    session.define_method(mrb, c"get", beni::method!(session_get, 1))?;
    session.define_method(mrb, c"put", beni::method!(session_put, 2))?;
    Ok(())
}

/// Mint the Session naming `id`, frozen so a script cannot re-point it.
///
/// `obj_new` is mruby's own constructor rather than the Ruby-level `new`
/// this class refuses, which is what leaves the refusal in place for
/// scripts while the wire still issues Handles.
pub(crate) fn mint(mrb: &Mrb, id: u32) -> Result<Value, Error> {
    use beni::IntoValue;

    let value = session_class(mrb)?.obj_new(mrb, &[(id as i32).into_value(mrb)])?;
    Ok(value.freeze(mrb))
}

/// The id `value` stands for, refusing anything that is not a Session.
///
/// Exact class rather than `kind_of?`: only `mint` produces one, so a
/// subclass is something a script built and derives no id.
pub(crate) fn handle_id(mrb: &Mrb, value: Value) -> Result<u32, Error> {
    use beni::FromValue;

    if !value.is_instance_of(mrb, session_class(mrb)?) {
        return Err(type_error(
            mrb,
            "expected a MyService::Session, which only MyService::KV.open hands out",
        ));
    }
    let id = value.iv_get(mrb, mrb.intern_cstr(HANDLE_IVAR));
    match i32::from_value(id) {
        Some(id) if id > 0 => Ok(id as u32),
        _ => Err(runtime_error(mrb, "this Session carries no Handle id")),
    }
}

fn session_class(mrb: &Mrb) -> Result<RClass, Error> {
    use beni::Module;

    // `define_module` returns the module already registered, so this is
    // a lookup rather than a second registration.
    mrb.define_module(c"MyService")?.class_get(mrb, c"Session")
}

fn initialize(mrb: &Mrb, self_: Value, id: i32) -> Result<Value, Error> {
    use beni::IntoValue;

    self_.iv_set(mrb, mrb.intern_cstr(HANDLE_IVAR), id.into_value(mrb))?;
    Ok(Value::nil())
}

// Any-arity because `method!`'s typed-parameter form has no `Value`
// identity conversion to ride, and the original this hook is handed is
// exactly a `Value`. It goes unread: mruby copies the ivar before the
// hook runs, so freezing the copy is all that is left to do.
fn initialize_copy(mrb: &Mrb, self_: Value) -> Result<Value, Error> {
    Ok(self_.freeze(mrb))
}

fn not_constructible(mrb: &Mrb, _self: Value) -> Value {
    let nomethod = mrb
        .exc_get(c"NoMethodError")
        .expect("NoMethodError is an mruby core class");
    // SAFETY: bridge frame — mruby unwinds through `mrb_raise`.
    unsafe {
        nomethod.raise(
            mrb,
            c"MyService::Session is a host-issued capability reference, not a constructible class",
        )
    }
}

/// `session.get(key)` — the stored String, or `nil` on a miss.
fn session_get(mrb: &Mrb, self_: Value, key: RString) -> Result<Value, Error> {
    dispatch::get(mrb, Target::Handle(handle_id(mrb, self_)?), key)
}

/// `session.put(key, value)` — `true` when the key already held a value.
fn session_put(mrb: &Mrb, self_: Value, key: RString, value: RString) -> Result<bool, Error> {
    dispatch::put(mrb, Target::Handle(handle_id(mrb, self_)?), key, value)
}
