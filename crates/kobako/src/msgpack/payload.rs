//! The `run` position: a payload spelled as a value tree.

use std::sync::Arc;

use kobako_codec::msgpack::codec::{Encode as _, Value};
use kobako_codec::msgpack::payload::Arguments;

use crate::error::{Error, Failure};
use crate::handles::Handles;
use crate::payload::RunPayload;
use crate::receiver::Receiver;

/// A `run` argument in the value-tree spelling: a `Value` passes by
/// value, a host object auto-wraps into a capability Handle the guest
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

/// Auto-wrap each host object into the invocation's Handle table, then
/// encode the pair.
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

/// Encode one argument, auto-wrapping a host object into the
/// invocation's Handle table. Exhaustion surfaces pre-call with the Ruby
/// counterpart's attribution — an outer `Err`, since the guest never ran.
fn wrap(handles: &Handles<'_>, arg: RunArg) -> Result<Value, Error> {
    match arg {
        RunArg::Value(value) => Ok(value),
        RunArg::Object(object) => handles.alloc(object).map(Value::Handle).map_err(|fault| {
            Error::Sandbox(Box::new(Failure {
                name: "Kobako::HandleExhaustedError".into(),
                message: fault.message,
                backtrace: Vec::new(),
                available: Vec::new(),
                diagnostic: None,
            }))
        }),
    }
}
