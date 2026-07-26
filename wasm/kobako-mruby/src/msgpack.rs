//! The MessagePack payload adapter — kobako's default schema for the
//! mruby guest, and what the bundled Guest Binary speaks.
//!
//! This is the whole of the guest's MessagePack surface: the transport
//! tier routes a Call without reading its payload, so a shell that names a
//! different `MrbGuest::Payload` leaves every byte below untouched. The
//! mruby ↔ wire value walk lives in the sibling `convert` module; this
//! file is the adapter's own face, framing those values into the payload
//! positions the wire contract defines.
//!
//! [payload adapter]: ../../../docs/wire/payload-msgpack.md

mod convert;

use beni::Value;
use kobako_codec::codec::{Decoder, Encode, Encoder, Value as CodecValue};
use kobako_codec::payload::Arguments;

use crate::adapter::{AdapterError, CallArguments, PayloadAdapter};
use crate::runtime::{Fault, Kobako};

/// kobako's default payload adapter.
pub struct MsgpackAdapter;

impl PayloadAdapter for MsgpackAdapter {
    fn encode_arguments(
        kobako: &Kobako,
        rest: &[Value],
        kwargs: beni::Hash,
    ) -> Result<Vec<u8>, AdapterError> {
        let (args, kwargs) = kobako.unpack_args_kwargs(rest, kwargs)?;
        Arguments::new(args, kwargs)
            .encode()
            .map_err(|_| AdapterError::Malformed)
    }

    fn decode_arguments(kobako: &Kobako, bytes: &[u8]) -> Result<CallArguments, AdapterError> {
        use kobako_codec::codec::Decode;
        let arguments = Arguments::decode(bytes).map_err(|_| AdapterError::Malformed)?;
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
            Some(kobako.to_mrb_value(CodecValue::Map(pairs))?)
        };
        Ok(CallArguments { args, kwargs })
    }

    fn encode_value(kobako: &Kobako, value: Value) -> Result<Vec<u8>, AdapterError> {
        let encoded = kobako
            .try_codec_value(value)
            .ok_or_else(|| AdapterError::unrepresentable(kobako, value))?;
        Encoder::encode(&encoded).map_err(|_| AdapterError::Malformed)
    }

    fn decode_value(kobako: &Kobako, bytes: &[u8]) -> Result<Value, AdapterError> {
        let value = Decoder::new(bytes)
            .read_only_value()
            .map_err(|_| AdapterError::Malformed)?;
        Ok(kobako.to_mrb_value(value)?)
    }

    fn decode_values(kobako: &Kobako, bytes: &[u8]) -> Result<Vec<Value>, AdapterError> {
        let CodecValue::Array(items) = Decoder::new(bytes)
            .read_only_value()
            .map_err(|_| AdapterError::Malformed)?
        else {
            return Err(AdapterError::Malformed);
        };
        items
            .into_iter()
            .map(|value| kobako.to_mrb_value(value))
            .collect::<Result<Vec<_>, _>>()
            .map_err(AdapterError::from)
    }

    fn decode_fault(bytes: &[u8]) -> Result<Fault, AdapterError> {
        convert::decode_fault(bytes).map_err(|_| AdapterError::Malformed)
    }
}
