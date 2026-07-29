//! The MessagePack payload codec — kobako's default schema for the
//! mruby guest, and what the bundled Guest Binary speaks.
//!
//! This is the whole of the guest's MessagePack surface: the transport
//! tier routes a Call without reading its payload, so a shell that names a
//! different `MrbGuest::Codec` leaves every byte below untouched. The
//! mruby ↔ wire value walk lives in the sibling `convert` module; this
//! file is the codec's own face, framing those values into the payload
//! positions the wire contract defines.
//!
//! [payload codec]: ../../../docs/wire/payload-msgpack.md

mod convert;

use beni::Value;
use kobako_codec::msgpack::codec::{Decoder, Encode, Encoder, Value as CodecValue};
use kobako_codec::msgpack::payload;

use crate::codec::{Arguments, CodecError, PayloadCodec};
use crate::runtime::Kobako;

/// kobako's default payload codec.
pub struct MsgpackCodec;

impl PayloadCodec for MsgpackCodec {
    fn encode_call_arguments(
        kobako: &Kobako,
        rest: &[Value],
        kwargs: beni::Hash,
    ) -> Result<Vec<u8>, CodecError> {
        let (args, kwargs) = kobako.unpack_args_kwargs(rest, kwargs)?;
        payload::Arguments::new(args, kwargs)
            .encode()
            .map_err(|_| CodecError::Malformed)
    }

    fn decode_run_arguments(kobako: &Kobako, bytes: &[u8]) -> Result<Arguments, CodecError> {
        use kobako_codec::msgpack::codec::Decode;
        let arguments = payload::Arguments::decode(bytes).map_err(|_| CodecError::Malformed)?;
        let args = arguments
            .args
            .into_iter()
            .map(|value| kobako.to_mrb_value(value))
            .collect::<Result<Vec<_>, _>>()?;
        // An empty keyword map stays absent so an entrypoint taking only
        // positionals never sees a trailing Hash it did not ask for.
        let kwargs = if arguments.kwargs.is_empty() {
            None
        } else {
            let pairs = arguments
                .kwargs
                .into_iter()
                .map(|(name, value)| (CodecValue::Sym(name), value))
                .collect();
            let hash = kobako.to_mrb_value(CodecValue::Map(pairs))?;
            // SAFETY: `to_mrb_value` builds a `CodecValue::Map` through
            // `mrb_hash_new`, so the value is Hash-tagged by construction.
            Some(unsafe { beni::Hash::from_value_unchecked(hash) })
        };
        Ok(Arguments { args, kwargs })
    }

    fn encode_value(kobako: &Kobako, value: Value) -> Result<Vec<u8>, CodecError> {
        let encoded = kobako
            .try_codec_value(value)
            .ok_or_else(|| CodecError::unrepresentable(kobako, value))?;
        Encoder::encode(&encoded).map_err(|_| CodecError::Malformed)
    }

    fn decode_reply_value(kobako: &Kobako, bytes: &[u8]) -> Result<Value, CodecError> {
        let value = Decoder::new(bytes)
            .read_only_value()
            .map_err(|_| CodecError::Malformed)?;
        Ok(kobako.to_mrb_value(value)?)
    }

    fn decode_yield_arguments(kobako: &Kobako, bytes: &[u8]) -> Result<Vec<Value>, CodecError> {
        let CodecValue::Array(items) = Decoder::new(bytes)
            .read_only_value()
            .map_err(|_| CodecError::Malformed)?
        else {
            return Err(CodecError::Malformed);
        };
        items
            .into_iter()
            .map(|value| kobako.to_mrb_value(value))
            .collect::<Result<Vec<_>, _>>()
            .map_err(CodecError::from)
    }
}
