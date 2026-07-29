//! The `MyService::KV` surface — the Service a script starts from.
//!
//! Each body is its own call site, so it knows which message it is
//! encoding before it reads an argument. That is the whole difference
//! from a self-describing payload: nothing on the wire has to say what
//! these bytes are, because both ends already agreed, and the envelope's
//! `(target, method)` pair is where they agreed it.
//!
//! `open` and `count` are the two ways a Handle travels. `open` answers
//! with one the host issued, and `count` passes one back in an argument
//! — a plain integer field, since an id is all a Handle is on the wire.

use beni::{Error, Mrb, RString, Value};
use kobako_mruby::Target;
use prost::Message;

use crate::blocks;
use crate::dispatch::{self, call, wire_error, KV_PATH};
use crate::schema::{CountRequest, CountResponse, OpenRequest, OpenResponse};
use crate::session;

/// Define `MyService::KV` and the Session class its `open` hands out.
pub(crate) fn init(mrb: &Mrb) -> Result<(), Error> {
    use beni::{Module, Object};

    let service = mrb.define_module(c"MyService")?;
    let kv = service.define_class(mrb, c"KV", mrb.object_class())?;
    kv.define_singleton_method(mrb, c"get", beni::method!(kv_get, 1))?;
    kv.define_singleton_method(mrb, c"put", beni::method!(kv_put, 2))?;
    kv.define_singleton_method(mrb, c"open", beni::method!(kv_open, 1))?;
    kv.define_singleton_method(mrb, c"count", beni::method!(kv_count, -1))?;
    kv.define_singleton_method(mrb, c"each_key", beni::method!(blocks::each_key, -1))?;
    session::init(mrb, service)
}

/// `MyService::KV.get(key)` — the stored String, or `nil` on a miss.
fn kv_get(mrb: &Mrb, _self: Value, key: RString) -> Result<Value, Error> {
    dispatch::get(mrb, Target::Path(KV_PATH), key)
}

/// `MyService::KV.put(key, value)` — `true` when the key already held a
/// value, `false` when it is new.
fn kv_put(mrb: &Mrb, _self: Value, key: RString, value: RString) -> Result<bool, Error> {
    dispatch::put(mrb, Target::Path(KV_PATH), key, value)
}

/// `MyService::KV.open(prefix)` — a Session scoped to `prefix`.
fn kv_open(mrb: &Mrb, _self: Value, prefix: RString) -> Result<Value, Error> {
    let request = OpenRequest {
        prefix: prefix.to_bytes(),
    };
    let body = call(
        mrb,
        Target::Path(KV_PATH),
        "open",
        request.encode_to_vec(),
        Value::nil(),
    )?;
    let answer =
        OpenResponse::decode(&body[..]).map_err(|err| wire_error(mrb, &err.to_string()))?;
    session::mint(mrb, answer.handle)
}

/// `MyService::KV.count(session)` — how many keys that Session wrote.
///
/// Any-arity because the argument is a Session object and `method!`'s
/// typed-parameter form has no `Value` identity conversion to ride.
fn kv_count(mrb: &Mrb, _self: Value) -> Result<i32, Error> {
    let session = mrb.get_args::<beni::format::O>();
    let request = CountRequest {
        handle: session::handle_id(mrb, session)?,
    };
    let body = call(
        mrb,
        Target::Path(KV_PATH),
        "count",
        request.encode_to_vec(),
        Value::nil(),
    )?;
    CountResponse::decode(&body[..])
        .map(|answer| answer.keys as i32)
        .map_err(|err| wire_error(mrb, &err.to_string()))
}
