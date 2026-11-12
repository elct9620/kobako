//! The `run` position: a payload spelled as a value tree.

use std::sync::Arc;

use kobako_codec::msgpack::codec::{Encode as _, Value};
use kobako_codec::msgpack::payload::Arguments;

use crate::error::{Error, Failure};
use crate::handles::Handles;
use crate::payload::RunPayload;
use crate::receiver::Receiver;

/// A `run` argument in the value-tree spelling: a `Value` passes by
/// value and carries no Handle, a host object auto-wraps into a capability Handle the guest
/// can call back into (the counterpart of the Ruby `#run` auto-wrap;
/// wrapping applies to the top-level argument position).
pub enum RunArg {
    Value(Value),
    Object(Arc<dyn Receiver>),
}

impl From<Value> for RunArg {
    fn from(value: Value) -> Self {
        RunArg::Value(value)
    }
}

impl RunPayload<'_> {
    /// The value-tree spelling of a run's arguments: positional and
    /// keyword arguments as wire values, with each host object
    /// auto-wrapped into a Handle from this invocation's table.
    pub fn values(args: Vec<RunArg>, kwargs: Vec<(String, RunArg)>) -> Self {
        RunPayload::build(move |handles| encode_values(handles, args, kwargs))
    }
}

fn encode_values(
    handles: &Handles<'_>,
    args: Vec<RunArg>,
    kwargs: Vec<(String, RunArg)>,
) -> Result<Vec<u8>, Error> {
    let args = args
        .into_iter()
        .map(|arg| wrap(handles, arg))
        .collect::<Result<_, _>>()?;
    let kwargs = kwargs
        .into_iter()
        .map(|(key, arg)| Ok((key, wrap(handles, arg)?)))
        .collect::<Result<_, Error>>()?;
    Arguments::new(args, kwargs)
        .encode()
        .map_err(|err| Error::Argument(format!("arguments are not wire-encodable: {err}")))
}

/// Table exhaustion surfaces as an outer `Err`, since the guest never ran.
/// A `Value::Handle` the Host App wrote is refused as an argument error,
/// since only this invocation's table issues the id a Handle carries.
fn wrap(handles: &Handles<'_>, arg: RunArg) -> Result<Value, Error> {
    match arg {
        RunArg::Value(value) if carries_handle(&value) => Err(Error::Argument(
            "a run argument cannot carry a Handle; pass the host object as RunArg::Object"
                .to_string(),
        )),
        RunArg::Value(value) => Ok(value),
        RunArg::Object(object) => handles.alloc(object).map(Value::Handle).map_err(|fault| {
            Error::Sandbox(Box::new(Failure::host(
                "Kobako::HandleExhaustedError",
                fault.message,
            )))
        }),
    }
}

fn carries_handle(value: &Value) -> bool {
    match value {
        Value::Handle(_) => true,
        Value::Array(items) => items.iter().any(carries_handle),
        Value::Map(pairs) => pairs
            .iter()
            .any(|(key, value)| carries_handle(key) || carries_handle(value)),
        _ => false,
    }
}
