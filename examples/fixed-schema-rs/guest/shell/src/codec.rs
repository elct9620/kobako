//! The payload schema this shell speaks.
//!
//! A fixed schema is chosen at a call site that knows which method it is
//! encoding for, and a `PayloadCodec` is handed no method name — so the
//! schema for a dispatch lives in the `kv` gem, not here. What is left
//! here are the positions no call site owns.
//!
//! Of those, the outcome is this codec's own: it carries a byte string
//! and nothing else, because a guest script hands back bytes it already
//! shaped and they need no schema beyond "these bytes, unchanged". The
//! entrypoint's arguments and a yield's are read by the gem, since the
//! schema is the gem's even where the context is not. And the two
//! positions that serve the dynamic proxy are simply not written: a codec
//! serves the positions it implements and refuses at the rest, so leaving
//! them alone is how this guest says it has no such path.

use beni::{FromValue, RString, Value};
use kobako_mruby::{Arguments, CodecError, Kobako, PayloadCodec};

/// The example's schema: a byte string in the outcome position.
pub(crate) struct RawBytes;

impl PayloadCodec for RawBytes {
    /// The `run` entrypoint's arguments — read by the gem, which owns
    /// the schema, not by this codec. The entrypoint convention is one
    /// `App.call(body, env)`, so there is one message to read and no
    /// name needed to choose it.
    ///
    /// `kwargs` stays `None`: a fixed schema names its arguments in the
    /// message, so it has no use for a keyword tail — which is just as
    /// well, since mruby's `funcall` family cannot mark one as keywords
    /// anyway.
    fn decode_run_arguments(kobako: &Kobako, bytes: &[u8]) -> Result<Arguments, CodecError> {
        match kv::entry::decode(kobako.mrb(), bytes) {
            Some(args) => Ok(Arguments { args, kwargs: None }),
            None => Err(CodecError::Malformed),
        }
    }

    /// The invocation's outcome — the only position this schema fills.
    /// A String travels as its own bytes, so a value the gem encoded
    /// reaches the host exactly as the gem wrote it, and a binary String
    /// is not re-encoded into something else on the way out.
    fn encode_value(kobako: &Kobako, value: Value) -> Result<Vec<u8>, CodecError> {
        RString::from_value(value)
            .map(RString::to_bytes)
            .ok_or_else(|| CodecError::unrepresentable(kobako, value))
    }

    /// A block's yielded arguments — read by the gem, which owns the
    /// schema. One shape per guest, for the same reason the entrypoint
    /// has one: nothing here says which method's block this is.
    fn decode_yield_arguments(kobako: &Kobako, bytes: &[u8]) -> Result<Vec<Value>, CodecError> {
        match kv::blocks::decode_yield(kobako.mrb(), bytes) {
            Some(args) => Ok(args),
            None => Err(CodecError::Malformed),
        }
    }
}
