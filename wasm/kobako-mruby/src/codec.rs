//! The payload codec seam — mruby values ↔ payload bytes.
//!
//! The transport tier routes a message without reading its payload
//! (`kobako_core::proxy`), so everything between the wire's
//! bytes and the interpreter's values happens through this trait. A shell
//! names its codec on `MrbGuest::Codec`; the flows never mention a
//! schema.
//!
//! The trait is `mrb_value` ↔ bytes rather than generic over a value type
//! because mruby is dynamically typed: there is no shape the guest could
//! parameterise on, so the codec is the only place that knows both
//! sides.
//!
//! [payload codec]: ../../../docs/wire/payload-msgpack.md

use beni::Value;

use crate::runtime::{IntegerOutOfRange, Kobako};

/// Why a codec could not carry a value across.
///
/// The kinds are distinct because each call site phrases them into its
/// own guest-visible failure: a yield raises `TypeError`, an outcome
/// writes a `Kobako::SandboxError` Panic, and a dispatch argument raises
/// `Kobako::Transport::Error` — all from the same refusal.
///
/// Non-exhaustive so a later kind does not break the codecs that match
/// on it; every variant here stays constructible by one.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CodecError {
    /// The value has no representation in this schema. Carries the mruby
    /// class name, which each call site templates into its own wording.
    Unrepresentable { type_name: String },
    /// Bytes this schema cannot read, or a value it cannot write.
    Malformed,
    /// The interpreter refused the value, not the schema. A codec reaches
    /// this by forwarding a refusal it did not make — the only place that
    /// conversion happens is inside a codec, so it travels on the codec's
    /// paths while naming whose refusal it is.
    Guest(IntegerOutOfRange),
}

impl CodecError {
    /// Name the mruby class this schema has no representation for.
    pub fn unrepresentable(kobako: &Kobako, value: Value) -> Self {
        CodecError::Unrepresentable {
            type_name: value.classname(kobako.mrb()),
        }
    }
}

impl From<IntegerOutOfRange> for CodecError {
    /// Lets a codec forward the interpreter's refusal with `?` — every
    /// codec builds its guest values through `Kobako`, so every codec
    /// meets this one.
    fn from(err: IntegerOutOfRange) -> Self {
        CodecError::Guest(err)
    }
}

/// A Call or Run payload as the interpreter sees it.
///
/// Positional and keyword arguments stay apart because the host dispatches
/// through `public_send`, where the two are not interchangeable — the one
/// shape obligation the wire contract puts on every codec.
pub struct Arguments {
    pub args: Vec<Value>,
    /// The keyword Hash, or `None` when the call passed no keywords — so
    /// an entrypoint taking only positionals never sees an empty Hash tail.
    pub kwargs: Option<Value>,
}

/// One schema for everything a core envelope hands through.
///
/// Every method is associated rather than taking `&self`: a codec is a
/// choice of encoding, not a value with state, and the flows reach it
/// through `G::Codec` with no instance to thread.
pub trait PayloadCodec {
    /// Encode a dispatch Call's arguments — the positional `rest` slice
    /// and the keyword Hash `mrb_get_args` separated out.
    fn encode_arguments(
        kobako: &Kobako,
        rest: &[Value],
        kwargs: beni::Hash,
    ) -> Result<Vec<u8>, CodecError>;

    /// Read a Call or Run payload back into interpreter values.
    fn decode_arguments(kobako: &Kobako, bytes: &[u8]) -> Result<Arguments, CodecError>;

    /// Encode one value — a dispatch return, an invocation outcome, or a
    /// block's result.
    fn encode_value(kobako: &Kobako, value: Value) -> Result<Vec<u8>, CodecError>;

    /// Read one value back — a Reply's ok body.
    fn decode_value(kobako: &Kobako, bytes: &[u8]) -> Result<Value, CodecError>;

    /// Read a Yield Call's arguments, which are a list rather than the
    /// args-and-kwargs pair a Call carries.
    fn decode_values(kobako: &Kobako, bytes: &[u8]) -> Result<Vec<Value>, CodecError>;
}
