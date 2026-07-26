//! The payload codec seam — mruby values ↔ payload bytes.
//!
//! The transport tier routes a message without reading its payload
//! (`kobako_core::transport::proxy`), so everything between the wire's
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

use std::sync::OnceLock;

use beni::Value;

use crate::runtime::{Fault, IntegerOutOfRange, Kobako};

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
    /// An inbound value has no faithful guest representation — an integer
    /// past what the MRB_INT32 build holds. Carries the operator-facing
    /// message, which is the same wherever it surfaces.
    OutOfRange { message: String },
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
    /// The MRB_INT32 refusal is the guest's, not the schema's, but it
    /// surfaces on the same paths — so it travels as a codec failure
    /// whichever codec was running.
    fn from(err: IntegerOutOfRange) -> Self {
        CodecError::OutOfRange {
            message: err.message(),
        }
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

    /// Read a Reply's fault body into the fields the guest raises with.
    /// Takes no `Kobako`: a fault becomes an exception, never a value.
    fn decode_fault(bytes: &[u8]) -> Result<Fault, CodecError>;
}

/// The three codec operations a guest→host dispatch needs.
///
/// The flows reach their codec as `G::Codec`, but the dispatch bridge
/// cannot: mruby invokes it through `mrb_func_t`, a bare function pointer
/// with nowhere to carry a type parameter. The shell's choice is recorded
/// here instead, once, when `Kobako::init` runs. A Guest Binary links
/// exactly one `MrbGuest`, so this can never come to disagree with
/// `G::Codec` — the write happens before any guest code runs and the
/// value never changes.
struct DispatchOps {
    encode_arguments: EncodeArgumentsFn,
    decode_value: DecodeValueFn,
    decode_fault: DecodeFaultFn,
}

type EncodeArgumentsFn = fn(&Kobako, &[Value], beni::Hash) -> Result<Vec<u8>, CodecError>;
type DecodeValueFn = fn(&Kobako, &[u8]) -> Result<Value, CodecError>;
type DecodeFaultFn = fn(&[u8]) -> Result<Fault, CodecError>;

static DISPATCH_OPS: OnceLock<DispatchOps> = OnceLock::new();

/// Record the guest's codec for the dispatch bridge. Called by
/// `Kobako::init`; later calls are ignored, since the first one already
/// named the only codec this binary has.
pub(crate) fn install_dispatch_ops<C: PayloadCodec>() {
    let _ = DISPATCH_OPS.set(DispatchOps {
        encode_arguments: C::encode_arguments,
        decode_value: C::decode_value,
        decode_fault: C::decode_fault,
    });
}

/// Encode a dispatch Call's arguments with the installed codec.
pub(crate) fn dispatch_encode_arguments(
    kobako: &Kobako,
    rest: &[Value],
    kwargs: beni::Hash,
) -> Result<Vec<u8>, CodecError> {
    (ops().encode_arguments)(kobako, rest, kwargs)
}

/// Read a Reply's ok body with the installed codec.
pub(crate) fn dispatch_decode_value(kobako: &Kobako, bytes: &[u8]) -> Result<Value, CodecError> {
    (ops().decode_value)(kobako, bytes)
}

/// Read a Reply's fault body with the installed codec.
pub(crate) fn dispatch_decode_fault(bytes: &[u8]) -> Result<Fault, CodecError> {
    (ops().decode_fault)(bytes)
}

/// The installed codec. A dispatch can only run inside an invocation,
/// which `Kobako::init` opens, so reaching here before the install is a
/// broken boot rather than a state to recover from.
fn ops() -> &'static DispatchOps {
    DISPATCH_OPS
        .get()
        .expect("Kobako::init installs the guest's payload codec before any dispatch")
}
