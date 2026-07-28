//! What a `#run` carries into the guest.
//!
//! A payload is either bytes that are already final, or a builder the
//! verb runs once the invocation's Handle table exists. The second form
//! is not a convenience: a payload naming a host object carries the id
//! that invocation's table issued for it, and that table does not exist
//! until the invocation has begun.
//!
//! The verb takes this type rather than a codec's shape, so which schema
//! spells a payload is a choice made here — at a constructor — instead of
//! at the verb. `values` is the bundled codec's spelling; a host with its
//! own schema builds its bytes and hands them over.

use std::sync::Mutex;

use crate::error::Error;
use crate::handles::{HandleTable, Handles};

#[cfg(feature = "msgpack")]
use kobako_codec::msgpack::codec::{Encode as _, Value};
#[cfg(feature = "msgpack")]
use kobako_codec::msgpack::payload::Arguments;
#[cfg(feature = "msgpack")]
use std::sync::Arc;

#[cfg(feature = "msgpack")]
use crate::error::Failure;
#[cfg(feature = "msgpack")]
use crate::receiver::Receiver;

/// A `run` argument in the value-tree spelling: a `Value` passes by
/// value, a host object auto-wraps into a capability Handle the guest
/// can call back into (the counterpart of the Ruby `#run` auto-wrap;
/// wrapping applies to the top-level argument position).
#[cfg(feature = "msgpack")]
pub enum RunArg {
    Value(Value),
    Object(Arc<dyn Receiver>),
}

#[cfg(feature = "msgpack")]
impl From<Value> for RunArg {
    fn from(value: Value) -> Self {
        RunArg::Value(value)
    }
}

/// The payload one `run` carries.
///
/// Opaque: how a payload reaches its bytes is this crate's business, so
/// a later way of producing them adds a constructor rather than changing
/// what callers match on.
pub struct RunPayload<'a>(Source<'a>);

/// The caller's step from this invocation's table to finished bytes.
type Finish<'a> = Box<dyn FnOnce(&Handles<'_>) -> Result<Vec<u8>, Error> + 'a>;

enum Source<'a> {
    /// Bytes the caller already holds — nothing in them names a host
    /// object, so no table is needed to finish them.
    Ready(Vec<u8>),
    /// Bytes the caller finishes once the invocation's table exists.
    Deferred(Finish<'a>),
}

impl<'a> RunPayload<'a> {
    /// A payload that is already complete.
    pub fn bytes(bytes: impl Into<Vec<u8>>) -> Self {
        RunPayload(Source::Ready(bytes.into()))
    }

    /// A payload built against this invocation's Handle table — the form
    /// to use when the entrypoint is handed a capability, since the id
    /// standing for it is issued by that table.
    pub fn build(build: impl FnOnce(&Handles<'_>) -> Result<Vec<u8>, Error> + 'a) -> Self {
        RunPayload(Source::Deferred(Box::new(build)))
    }

    /// The value-tree spelling of a run's arguments: positional and
    /// keyword arguments as wire values, with each host object
    /// auto-wrapped into a Handle from this invocation's table.
    ///
    /// Gated on `msgpack` because that is where the `Value` type
    /// currently lives, not because the shape is MessagePack's.
    #[cfg(feature = "msgpack")]
    pub fn values(args: Vec<RunArg>, kwargs: Vec<(String, RunArg)>) -> Self {
        RunPayload::build(move |handles| encode_values(handles, args, kwargs))
    }

    /// Finish the payload against the invocation's table.
    pub(crate) fn encode(self, table: &Mutex<HandleTable>) -> Result<Vec<u8>, Error> {
        match self.0 {
            Source::Ready(bytes) => Ok(bytes),
            Source::Deferred(build) => build(&Handles::new(table)),
        }
    }
}

/// Auto-wrap each host object into the invocation's Handle table, then
/// encode the pair.
#[cfg(feature = "msgpack")]
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
#[cfg(feature = "msgpack")]
fn wrap(handles: &Handles<'_>, arg: RunArg) -> Result<Value, Error> {
    match arg {
        RunArg::Value(value) => Ok(value),
        RunArg::Object(object) => handles.alloc(object).map(Value::Handle).map_err(|fault| {
            Error::Sandbox(Box::new(Failure {
                class: "Kobako::HandleExhaustedError".into(),
                message: fault.message,
                backtrace: Vec::new(),
                available: Vec::new(),
                diagnostic: None,
            }))
        }),
    }
}
