//! Module-level static slot naming the guest's payload codec for the
//! dispatch bridge.
//!
//! The flows reach their codec as `G::Codec`, but the bridge cannot:
//! mruby invokes it through `mrb_func_t`, a bare function pointer with
//! nowhere to carry a type parameter. The shell's choice is recorded here
//! instead, once, when `Kobako::init` runs — the same constraint and the
//! same shape as `flows::mrb_slot`, whose cross-invocation isolation
//! argument applies here unchanged.

use std::sync::OnceLock;

use beni::Value;

use crate::codec::{CodecError, PayloadCodec};
use crate::runtime::Kobako;

type EncodeCallArgumentsFn = fn(&Kobako, &[Value], beni::Hash) -> Result<Vec<u8>, CodecError>;
type DecodeReplyValueFn = fn(&Kobako, &[u8]) -> Result<Value, CodecError>;

/// The two codec operations a guest→host dispatch needs, as plain
/// function pointers. A Guest Binary links exactly one `MrbGuest`, so the
/// slot can never come to disagree with `G::Codec` — the write happens
/// before any guest code runs and the value never changes.
pub(crate) struct CodecSlot {
    encode_call_arguments: EncodeCallArgumentsFn,
    decode_reply_value: DecodeReplyValueFn,
}

impl CodecSlot {
    /// Encode a dispatch Call's arguments.
    pub(crate) fn encode_call_arguments(
        &self,
        kobako: &Kobako,
        rest: &[Value],
        kwargs: beni::Hash,
    ) -> Result<Vec<u8>, CodecError> {
        (self.encode_call_arguments)(kobako, rest, kwargs)
    }

    /// Read a Reply's ok body.
    pub(crate) fn decode_reply_value(
        &self,
        kobako: &Kobako,
        bytes: &[u8],
    ) -> Result<Value, CodecError> {
        (self.decode_reply_value)(kobako, bytes)
    }
}

static SLOT: OnceLock<CodecSlot> = OnceLock::new();

/// Record the guest's codec. Called by `Kobako::init`; later calls are
/// ignored, since the first one already named the only codec this binary
/// has.
pub(crate) fn install<C: PayloadCodec>() {
    let _ = SLOT.set(CodecSlot {
        encode_call_arguments: C::encode_call_arguments,
        decode_reply_value: C::decode_reply_value,
    });
}

/// The installed codec. A dispatch can only run inside an invocation,
/// which `Kobako::init` opens, so reaching here before the install is a
/// broken boot rather than a state to recover from.
pub(crate) fn get() -> &'static CodecSlot {
    SLOT.get()
        .expect("Kobako::init installs the guest's payload codec before any dispatch")
}
