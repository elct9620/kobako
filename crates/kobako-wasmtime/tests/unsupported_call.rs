//! A host frontend of its own may answer a guest's Call as unsupported;
//! the bundled guest must then hand its script a refusal it can rescue,
//! rather than a broken exchange. The default frontends serve every Call,
//! so only a handler written here reaches that answer.

mod support;

use std::sync::Arc;

use kobako_runtime::{Completion, DispatchHandler, Entry, Runtime, Yielder};
use kobako_transport::envelope::{Bindings, Call, Outcome, Preamble, Reply, Unsupported};

struct ServesNoCall;

impl DispatchHandler for ServesNoCall {
    fn dispatch(&self, _call: Call<'_>, _yielder: &mut dyn Yielder) -> Option<Reply> {
        Some(Reply::Unsupported(Unsupported::new("no calls here")))
    }
}

// @behavior T-279
#[test]
fn a_call_the_host_does_not_serve_raises_unsupported_error_in_the_guest() {
    let Some(driver) = support::bundled_driver() else {
        return;
    };
    let mut preamble = Preamble::default();
    preamble.bindings = Bindings {
        paths: vec!["Svc".into()],
    };
    let source = b"begin; Svc.go; rescue Kobako::UnsupportedError => e; e.message; end";

    let snapshot = driver
        .invoke(
            Entry::Eval { source },
            &preamble,
            Some(Arc::new(ServesNoCall)),
            &[],
        )
        .expect("the invocation must start");

    let Completion::Outcome(outcome) = snapshot.completion else {
        panic!("the invocation must answer rather than trap");
    };
    assert_eq!(
        outcome,
        Outcome::Ok(b"\xadno calls here".to_vec()),
        "a Service call answered as unsupported through Driver#invoke must raise \
         Kobako::UnsupportedError carrying the host's message"
    );
}
