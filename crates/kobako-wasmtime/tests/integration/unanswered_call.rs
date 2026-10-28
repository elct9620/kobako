//! A Call the host could not answer as the Service meant still reaches the
//! guest as an answer it can rescue, and the answer says why. The default
//! frontends never hand the driver either failure, so only a handler
//! written here reaches them.

use crate::support;

use std::sync::Arc;

use kobako_runtime::{Completion, DispatchHandler, Entry, Runtime, Yielder};
use kobako_transport::envelope::{Bindings, Call, Outcome, Preamble, Reply};

/// Answers every Call with one byte more than a message carries.
struct AnswersTooMuch;

impl DispatchHandler for AnswersTooMuch {
    fn dispatch(&self, _call: Call<'_>, _yielder: &mut dyn Yielder) -> Option<Reply> {
        Some(Reply::Ok(vec![0xc0; (16 << 20) + 1]))
    }
}

/// Breaks the handler contract by answering nothing.
struct AnswersNothing;

impl DispatchHandler for AnswersNothing {
    fn dispatch(&self, _call: Call<'_>, _yielder: &mut dyn Yielder) -> Option<Reply> {
        None
    }
}

fn eval_against(handler: Arc<dyn DispatchHandler>, source: &[u8]) -> Option<Outcome> {
    let driver = support::bundled_driver()?;
    let mut preamble = Preamble::default();
    preamble.bindings = Bindings {
        paths: vec!["Svc".into()],
    };
    let snapshot = driver
        .invoke(Entry::Eval { source }, &preamble, Some(handler), &[])
        .expect("the invocation must start");
    let Completion::Outcome(outcome) = snapshot.completion else {
        panic!("the invocation must answer rather than trap");
    };
    Some(outcome)
}

// @behavior T-286
#[test]
fn an_answer_larger_than_a_message_raises_a_service_failure_naming_the_limit() {
    let source = b"begin; Svc.go; rescue Kobako::ServiceError => e; e.message; end";
    let Some(outcome) = eval_against(Arc::new(AnswersTooMuch), source) else {
        return;
    };

    let mut expected = vec![0xd9, 0x4e];
    expected.extend_from_slice(
        b"Sandbox could not write the Service's answer: payload exceeds the 16 MiB limit",
    );
    assert_eq!(
        outcome,
        Outcome::Ok(expected),
        "a Service answer past the message cap through Driver#invoke must raise a Service \
         failure in the guest naming the limit"
    );
}

// @behavior T-287
#[test]
fn a_call_the_host_could_not_answer_raises_a_transport_error_naming_why() {
    let source = b"begin; Svc.go; rescue Kobako::Transport::Error => e; e.message; end";
    let Some(outcome) = eval_against(Arc::new(AnswersNothing), source) else {
        return;
    };

    let mut expected = vec![0xd9, 0x6a];
    expected.extend_from_slice(
        "a Sandbox callback raised an exception instead of returning a fault — please report \
         this as a kobako bug"
            .as_bytes(),
    );
    assert_eq!(
        outcome,
        Outcome::Ok(expected),
        "a Call a handler answered with nothing through Driver#invoke must raise \
         Kobako::Transport::Error in the guest naming why"
    );
}
