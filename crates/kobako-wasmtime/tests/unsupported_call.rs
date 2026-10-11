//! A host frontend of its own may answer a guest's Call as unsupported;
//! the bundled guest must then hand its script a refusal it can rescue,
//! rather than a broken exchange. The default frontends serve every Call,
//! so only a handler written here reaches that answer.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use kobako_runtime::{Completion, DispatchHandler, Entry, Profile, Runtime, Yielder};
use kobako_transport::envelope::{Bindings, Call, Frames, Outcome, Reply, Snippets, Unsupported};
use kobako_wasmtime::{Config, Driver};

const WASM: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/kobako.wasm");

struct ServesNoCall;

impl DispatchHandler for ServesNoCall {
    fn dispatch(&self, _call: Call<'_>, _yielder: &mut dyn Yielder) -> Option<Reply> {
        Some(Reply::Unsupported(Unsupported::new("no calls here")))
    }
}

// @behavior T-279
#[test]
fn a_call_the_host_does_not_serve_raises_unsupported_error_in_the_guest() {
    if !Path::new(WASM).exists() {
        assert!(
            std::env::var_os("CI").is_none(),
            "data/kobako.wasm missing under CI — run `bundle exec rake wasm:build`"
        );
        return;
    }
    let driver = Driver::new(
        Path::new(WASM),
        Config {
            timeout: Some(Duration::from_secs(10)),
            memory_limit: None,
            stdout_limit: None,
            stderr_limit: None,
            profile: Profile::Hermetic,
        },
    )
    .expect("the Guest Binary must load");
    let frames = Frames {
        bindings: Bindings {
            paths: vec!["Svc".into()],
        },
        snippets: Snippets {
            entries: Vec::new(),
        },
    };
    let source = b"begin; Svc.go; rescue Kobako::UnsupportedError => e; e.message; end";

    let snapshot = driver
        .invoke(
            Entry::Eval { source },
            &frames,
            Some(Arc::new(ServesNoCall)),
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
