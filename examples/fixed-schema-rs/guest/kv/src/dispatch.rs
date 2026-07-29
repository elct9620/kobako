//! Reaching the host: where this gem dispatches, one Call out and one
//! answer back, and the two operations both of its surfaces expose.
//!
//! The target is a parameter because that is the only thing the two ways
//! of reaching a host object differ in — a bound Service is a path, a
//! capability Handle is the id the table issued. The schema, the method
//! name, and the refusal handling are the same either way, so they are
//! written once here and the surfaces supply the target.

use beni::{Error, Mrb, RString, Value};
use kobako_mruby::{dispatch, DispatchError, Target};
use prost::Message;

use crate::schema::{GetRequest, GetResponse, PutRequest, PutResponse};

/// The bound path this gem's Service methods dispatch to. The host binds
/// a receiver at the same path and reads the method name off the Call to
/// pick the message — the pair is the schema key on both sides.
pub(crate) const KV_PATH: &str = "MyService::KV";

/// Round one Call through the host and hand back the ok body. `block` is
/// what the calling method received — `Value::nil()` when it took none,
/// and the harness parks it for the call's duration either way, so this
/// gem states a block once rather than twice.
///
/// A refusal arrives typed on the envelope rather than as payload bytes,
/// so this gem reads the host's reason for saying no without owning a
/// representation for it — and without linking one.
pub(crate) fn call(
    mrb: &Mrb,
    target: Target<'_>,
    method: &str,
    payload: Vec<u8>,
    block: Value,
) -> Result<Vec<u8>, Error> {
    dispatch(target, method, block, &payload).map_err(|err| match err {
        DispatchError::Fault(fault) => runtime_error(
            mrb,
            &format!(
                "MyService.{method} was refused ({}): {}",
                fault.kind.name(),
                fault.message
            ),
        ),
        // The wildcard is the point of a non-exhaustive error: a way for
        // the exchange to fail that this gem was written before still
        // reaches the script, carrying whatever the transport said.
        other => wire_error(mrb, &other.to_string()),
    })
}

/// `get(key)` against `target` — the stored String, or `nil` on a miss.
pub(crate) fn get(mrb: &Mrb, target: Target<'_>, key: RString) -> Result<Value, Error> {
    let request = GetRequest {
        key: key.to_bytes(),
    };
    let body = call(mrb, target, "get", request.encode_to_vec(), Value::nil())?;
    let answer = GetResponse::decode(&body[..]).map_err(|err| wire_error(mrb, &err.to_string()))?;
    Ok(if answer.found {
        mrb.str_new(&answer.value).as_value()
    } else {
        Value::nil()
    })
}

/// `put(key, value)` against `target` — `true` when the key already held
/// a value, `false` when it is new.
pub(crate) fn put(
    mrb: &Mrb,
    target: Target<'_>,
    key: RString,
    value: RString,
) -> Result<bool, Error> {
    let request = PutRequest {
        key: key.to_bytes(),
        value: value.to_bytes(),
    };
    let body = call(mrb, target, "put", request.encode_to_vec(), Value::nil())?;
    PutResponse::decode(&body[..])
        .map(|answer| answer.replaced)
        .map_err(|err| wire_error(mrb, &err.to_string()))
}

/// A schema-level failure: bytes this gem's schema cannot read.
pub(crate) fn wire_error(mrb: &Mrb, message: &str) -> Error {
    runtime_error(mrb, &format!("KV wire error: {message}"))
}

pub(crate) fn runtime_error(mrb: &Mrb, message: &str) -> Error {
    match mrb.exc_get(c"RuntimeError") {
        Ok(class) => Error::new(mrb, class, message),
        Err(err) => err,
    }
}

/// A guest-side type refusal, raised before anything reaches the wire.
pub(crate) fn type_error(mrb: &Mrb, message: &str) -> Error {
    match mrb.exc_get(c"TypeError") {
        Ok(class) => Error::new(mrb, class, message),
        Err(err) => err,
    }
}
