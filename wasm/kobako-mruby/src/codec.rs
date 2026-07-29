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
//! Each method names the envelope position it serves rather than the guest
//! concept behind it, because a replacement codec is written against the
//! byte-level contract those positions are defined in. A codec serves the
//! positions it implements and refuses at the rest, so the set of methods
//! it writes is the set of capabilities it offers.
//!
//! [payload codec]: ../../../docs/wire/payload-msgpack.md

use crate::runtime::{IntegerOutOfRange, Kobako};

/// Why a codec could not carry a value across.
///
/// The kinds are distinct because each call site phrases them into its
/// own guest-visible failure, and the class follows what the refusal says
/// happened: a value with no representation raises `TypeError` wherever
/// the script handed it over, while bytes that could not be read are a
/// `Kobako::Transport::Error` — or, where no guest frame is running to
/// receive one, a `Kobako::SandboxError` Panic.
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
    Interpreter(IntegerOutOfRange),
    /// This codec does not serve the position it was asked at. Distinct
    /// from `Malformed` because nothing was wrong with the bytes or the
    /// value: the capability is absent, and a reader that cannot tell the
    /// two apart cannot tell a broken message from a guest that never
    /// offered the feature.
    Unsupported,
}

impl CodecError {
    /// Name the mruby class this schema has no representation for.
    pub fn unrepresentable(kobako: &Kobako, value: beni::Value) -> Self {
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
        CodecError::Interpreter(err)
    }
}

impl std::fmt::Display for CodecError {
    /// The refusal without the position it happened at. Each call site
    /// still phrases its own guest-visible wording; this is what a codec
    /// author sees when the refusal reaches their own error reporting.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CodecError::Unrepresentable { type_name } => {
                write!(f, "{type_name} has no representation in this schema")
            }
            CodecError::Malformed => f.write_str("bytes this schema cannot read"),
            CodecError::Interpreter(err) => f.write_str(&err.message()),
            CodecError::Unsupported => f.write_str("a position this schema does not serve"),
        }
    }
}

impl std::error::Error for CodecError {}

/// A Run payload as the interpreter sees it.
///
/// Positional and keyword arguments stay apart because the entrypoint is
/// called through `mrb_funcall_argv`, where the keyword Hash rides as the
/// trailing positional — so it has to arrive already separated to be
/// appended last, and typed as a Hash so nothing else can take that slot.
pub struct Arguments {
    pub args: Vec<beni::Value>,
    /// The keyword Hash, or `None` when the call passed no keywords — so
    /// an entrypoint taking only positionals never sees an empty Hash tail.
    pub kwargs: Option<beni::Hash>,
}

/// One schema for everything a core envelope hands through.
///
/// Every method is associated rather than taking `&self`: a codec is a
/// choice of encoding, not a value with state, and the flows reach it
/// through `G::Codec` with no instance to thread.
///
/// `encode_value` is the capability floor and the only required method:
/// every invocation ends by writing one Outcome, so a codec that cannot
/// write a value cannot complete anything. The other four positions are
/// optional — a codec that leaves one alone refuses there, and the call
/// site reports the absence rather than a broken message.
pub trait PayloadCodec {
    /// Write a value for the two positions that carry one: the Outcome's
    /// ok arm and a Yield Reply's ok / break body.
    fn encode_value(kobako: &Kobako, value: beni::Value) -> Result<Vec<u8>, CodecError>;

    /// Write the Call payload of a guest→host dispatch — the positional
    /// `rest` slice and the keyword Hash `mrb_get_args` separated out.
    ///
    /// Paired with `decode_reply_value`: a codec that serves one half of
    /// a dispatch owes the other. Nothing enforces that, so a codec that
    /// writes a Call it cannot read the answer to leaves the exchange
    /// half-served at the Reply.
    fn encode_call_arguments(
        kobako: &Kobako,
        rest: &[beni::Value],
        kwargs: beni::Hash,
    ) -> Result<Vec<u8>, CodecError> {
        let _ = (kobako, rest, kwargs);
        Err(CodecError::Unsupported)
    }

    /// Read the ok body of the Reply that dispatch came back with.
    fn decode_reply_value(kobako: &Kobako, bytes: &[u8]) -> Result<beni::Value, CodecError> {
        let _ = (kobako, bytes);
        Err(CodecError::Unsupported)
    }

    /// Read the Run payload — the arguments an invocation's entrypoint is
    /// called with.
    fn decode_run_arguments(kobako: &Kobako, bytes: &[u8]) -> Result<Arguments, CodecError> {
        let _ = (kobako, bytes);
        Err(CodecError::Unsupported)
    }

    /// Read a Yield Call's arguments, which are a plain list rather than
    /// the args-and-kwargs pair a Call carries.
    fn decode_yield_arguments(
        kobako: &Kobako,
        bytes: &[u8],
    ) -> Result<Vec<beni::Value>, CodecError> {
        let _ = (kobako, bytes);
        Err(CodecError::Unsupported)
    }
}
