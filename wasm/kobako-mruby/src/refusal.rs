//! What a codec refusal becomes at the position it happened.
//!
//! A codec reports only what it could not do. The class and wording that
//! refusal reaches its reader as follow from the position instead, because
//! the position is what fixes the direction the value was travelling and
//! who is listening at the other end.
//!
//! The mapping lives here, apart from the interpreter, so every position
//! can be pinned by a host-side test — the flows that raise and panic from
//! it only compile against a linked mruby.

use crate::codec::CodecError;

/// The wire-layer error class an exchange that did not complete carries.
const TRANSPORT_ERROR: &str = "Kobako::Transport::Error";

/// A place a payload codec is asked to carry a value across.
///
/// Two of these — a block's return and its `break` value — share one
/// envelope position and differ only in the value they name, because the
/// reader is told which of the two it was handed.
///
/// Only the two dispatch positions are named outside the invocation
/// flows, and those flows need a linked mruby to compile — so a
/// placeholder build sees the rest constructed by this module's tests
/// alone.
#[cfg_attr(not(mruby_linked), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Position {
    /// A guest→host dispatch's arguments, written by the calling script.
    CallArguments,
    /// The value a dispatch Reply came back with.
    ReplyValue,
    /// The arguments an invocation's entrypoint is called with.
    RunArguments,
    /// The arguments a block is yielded.
    YieldArguments,
    /// The value a block returned.
    BlockReturnValue,
    /// The value a block `break`ed with.
    BreakValue,
    /// The value one invocation finished with.
    InvocationValue,
}

/// The failure a refusal reaches its reader as: an error class, and the
/// wording that class carries. How it is delivered — raised in the guest
/// frame, framed as a Panic, or written to a Yield Reply's error arm — is
/// the position's own business.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Refusal {
    pub class: &'static str,
    pub message: String,
}

impl Refusal {
    /// The exchange did not complete, whatever stopped it.
    fn transport(message: impl Into<String>) -> Self {
        Refusal {
            class: TRANSPORT_ERROR,
            message: message.into(),
        }
    }
}

impl Position {
    /// What this position calls the value it carries, as the reader's
    /// wording names it.
    fn value_name(self) -> &'static str {
        match self {
            Position::CallArguments => "argument",
            Position::ReplyValue | Position::InvocationValue => "return value",
            Position::RunArguments => "invocation argument",
            Position::YieldArguments => "block argument",
            Position::BlockReturnValue => "block return value",
            Position::BreakValue => "break value",
        }
    }

    /// Who a value with no representation in the schema is reported to.
    /// A script that handed one over made a type error; where no guest
    /// frame is running to hear that, the invocation failed instead.
    fn unrepresentable_class(self) -> &'static str {
        match self {
            Position::CallArguments
            | Position::ReplyValue
            | Position::BlockReturnValue
            | Position::BreakValue => "TypeError",
            Position::InvocationValue => "Kobako::SandboxError",
            Position::RunArguments | Position::YieldArguments => TRANSPORT_ERROR,
        }
    }

    /// What this position says when the bytes themselves were the problem.
    fn unreadable_message(self) -> &'static str {
        match self {
            Position::CallArguments | Position::ReplyValue => {
                "transport envelope error (proxy dispatch)"
            }
            Position::RunArguments => "failed to decode the invocation arguments",
            Position::YieldArguments => "failed to read the block argument",
            Position::BlockReturnValue => "failed to read the block return value",
            Position::BreakValue => "failed to read the break value",
            Position::InvocationValue => "result envelope encode failed",
        }
    }
}

/// Attribute one codec refusal to the position that provoked it.
pub(crate) fn at(position: Position, err: CodecError) -> Refusal {
    // A `#run` payload the codec could not read answers the same way
    // whatever it reported: the invocation never started, so there is no
    // reader the distinction would reach.
    if position == Position::RunArguments {
        return Refusal::transport(position.unreadable_message());
    }

    match err {
        CodecError::Unrepresentable { type_name } => Refusal {
            class: position.unrepresentable_class(),
            message: format!(
                "{} of type {type_name} is not a supported sandbox value type",
                position.value_name()
            ),
        },
        CodecError::Interpreter(err) => Refusal::transport(err.message()),
        CodecError::Malformed => Refusal::transport(position.unreadable_message()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::IntegerOutOfRange;

    const EVERY_POSITION: [Position; 7] = [
        Position::CallArguments,
        Position::ReplyValue,
        Position::RunArguments,
        Position::YieldArguments,
        Position::BlockReturnValue,
        Position::BreakValue,
        Position::InvocationValue,
    ];

    fn unrepresentable() -> CodecError {
        CodecError::Unrepresentable {
            type_name: "Proc".into(),
        }
    }

    // A script hands a value over at a position a guest frame is running
    // at; the refusal has to read as that script's own mistake, which is
    // the class Ruby already uses for handing over the wrong type.
    #[test]
    fn a_value_the_schema_cannot_write_is_the_script_s_type_error() {
        for position in [
            Position::CallArguments,
            Position::ReplyValue,
            Position::BlockReturnValue,
            Position::BreakValue,
        ] {
            assert_eq!(
                at(position, unrepresentable()).class,
                "TypeError",
                "an unrepresentable value at a position a script handed it to must \
                 refuse as that script's own type error"
            );
        }
    }

    // No guest frame is left to receive an exception once the invocation
    // is writing its own outcome, so the same fact has to reach the host
    // as the invocation failing.
    #[test]
    fn an_unrepresentable_invocation_value_fails_the_invocation() {
        let refusal = at(Position::InvocationValue, unrepresentable());

        assert_eq!(
            refusal.class, "Kobako::SandboxError",
            "an unrepresentable invocation value must refuse as the invocation failing, \
             since no guest frame is left to raise into"
        );
        assert_eq!(
            refusal.message, "return value of type Proc is not a supported sandbox value type",
            "the refusal must name the type the codec reported, so a developer sees which \
             one failed without an implicit inspect"
        );
    }

    // The interpreter's own limit is not the schema's, and it is not the
    // script's either — the value simply could not survive the crossing.
    #[test]
    fn the_interpreter_s_own_refusal_travels_as_a_wire_fault_everywhere() {
        for position in EVERY_POSITION {
            let refusal = at(
                position,
                CodecError::Interpreter(IntegerOutOfRange(1 << 40)),
            );

            assert_eq!(
                refusal.class, TRANSPORT_ERROR,
                "an interpreter limit at any position must refuse as an exchange that did \
                 not complete, not as a schema or script fault"
            );
        }
    }

    // Bytes that could not be read say nothing about a value, so each
    // position answers with what it was trying to read. The two dispatch
    // positions share one wording — both halves of the same exchange fail
    // at the same guest call site, so a script cannot act on which half.
    #[test]
    fn unreadable_bytes_name_the_exchange_that_could_not_read_them() {
        let dispatch = "transport envelope error (proxy dispatch)";
        for (position, expected) in [
            (Position::CallArguments, dispatch),
            (Position::ReplyValue, dispatch),
            (
                Position::RunArguments,
                "failed to decode the invocation arguments",
            ),
            (
                Position::YieldArguments,
                "failed to read the block argument",
            ),
            (
                Position::BlockReturnValue,
                "failed to read the block return value",
            ),
            (Position::BreakValue, "failed to read the break value"),
            (Position::InvocationValue, "result envelope encode failed"),
        ] {
            assert_eq!(
                at(position, CodecError::Malformed).message,
                expected,
                "unreadable bytes at {position:?} must name the exchange that could not \
                 read them"
            );
        }
    }

    // A `#run` payload is read before anything is running, so the four
    // kinds have no reader to tell apart and collapse to one answer.
    #[test]
    fn a_run_payload_refuses_the_same_way_whatever_the_codec_reported() {
        let refusals = [
            at(Position::RunArguments, unrepresentable()),
            at(
                Position::RunArguments,
                CodecError::Interpreter(IntegerOutOfRange(1 << 40)),
            ),
            at(Position::RunArguments, CodecError::Malformed),
        ];

        assert!(
            refusals.iter().all(|r| *r == refusals[0]),
            "a run payload the codec could not read must answer identically for every kind, \
             since the invocation never started and no reader would see a distinction"
        );
    }
}
