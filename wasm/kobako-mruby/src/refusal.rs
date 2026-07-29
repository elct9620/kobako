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

    /// Who an absent capability is reported to. A script that reached a
    /// position its own guest does not serve asked for something
    /// unimplemented; where no guest frame is listening, the invocation
    /// failed instead. A Reply is the exception: a codec that wrote the
    /// Call owes its answer, so refusing there leaves the exchange
    /// half-served rather than a feature unoffered.
    fn unsupported_class(self) -> &'static str {
        match self {
            Position::CallArguments
            | Position::YieldArguments
            | Position::BlockReturnValue
            | Position::BreakValue => "NotImplementedError",
            Position::RunArguments | Position::InvocationValue => "Kobako::SandboxError",
            Position::ReplyValue => TRANSPORT_ERROR,
        }
    }
}

/// Attribute one codec refusal to the position that provoked it.
pub(crate) fn at(position: Position, err: CodecError) -> Refusal {
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
        CodecError::Unsupported => Refusal {
            class: position.unsupported_class(),
            message: format!(
                "this guest's schema does not serve the {} position",
                position.value_name()
            ),
        },
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

    // A `#run` payload is read before anything of the invocation runs, so
    // its report is the only account of the failure a host ever gets.
    #[test]
    fn a_run_payload_reports_what_the_codec_reported() {
        let refusals = [
            at(
                Position::RunArguments,
                CodecError::Interpreter(IntegerOutOfRange(1 << 40)),
            ),
            at(Position::RunArguments, CodecError::Malformed),
            at(Position::RunArguments, CodecError::Unsupported),
        ];
        let messages: Vec<_> = refusals.iter().map(|r| r.message.as_str()).collect();

        assert_eq!(
            messages.len(),
            messages
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len(),
            "a run payload must word each kind distinctly — the Panic it becomes is the \
             host's only account of why the invocation never started: {messages:?}"
        );
    }

    // A script asked for a position this schema does not serve. That is
    // not the value's fault and not the wire's — the capability is simply
    // absent, which is the one thing a bare `rescue` should not swallow.
    #[test]
    fn a_position_the_schema_does_not_serve_refuses_as_unimplemented() {
        for position in [
            Position::CallArguments,
            Position::YieldArguments,
            Position::BlockReturnValue,
            Position::BreakValue,
        ] {
            assert_eq!(
                at(position, CodecError::Unsupported).class,
                "NotImplementedError",
                "a position this schema does not serve must refuse as unimplemented at \
                 {position:?}, so a script's bare rescue does not swallow a missing capability"
            );
        }
    }

    // A codec that wrote the Call but cannot read the Reply left the
    // exchange half-served. Nothing is missing from the script's point of
    // view — the guest was assembled inconsistently.
    #[test]
    fn a_reply_the_schema_does_not_serve_is_an_incomplete_exchange() {
        assert_eq!(
            at(Position::ReplyValue, CodecError::Unsupported).class,
            TRANSPORT_ERROR,
            "a schema that serves a Call but not its Reply must refuse as an exchange that \
             did not complete, since the guest was assembled with half a dispatch"
        );
    }

    // With no guest frame left to raise into, an absent capability can
    // only be reported as the invocation failing.
    #[test]
    fn an_absent_capability_the_host_reads_fails_the_invocation() {
        for position in [Position::RunArguments, Position::InvocationValue] {
            assert_eq!(
                at(position, CodecError::Unsupported).class,
                "Kobako::SandboxError",
                "a position only the host reads must refuse as the invocation failing at \
                 {position:?}, since no guest frame is left to raise into"
            );
        }
    }

    // `encode_value` is the capability floor, so nothing stops a codec
    // returning `Unsupported` from it anyway — the two positions it serves
    // still need a real answer rather than an unreachable assertion.
    #[test]
    fn the_floor_s_own_positions_still_answer_an_unsupported_refusal() {
        for position in [
            Position::BlockReturnValue,
            Position::BreakValue,
            Position::InvocationValue,
        ] {
            assert!(
                !at(position, CodecError::Unsupported).message.is_empty(),
                "a codec refusing at {position:?} must still be reported, since the trait \
                 requires the method but cannot forbid the refusal"
            );
        }
    }
}
