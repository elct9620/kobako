//! The bundled guest recognises no flag, so every flag a host sets meets
//! the unrecognised path: a critical one must refuse the invocation under
//! its own class, and an advisory one must leave the run untouched.

use std::path::Path;
use std::time::Duration;

use kobako_runtime::{Completion, Entry, Profile, Runtime};
use kobako_transport::envelope::{Flag, Origin, Outcome, Preamble};
use kobako_wasmtime::{Config, Driver};

const WASM: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/kobako.wasm");

/// The Outcome and what the script printed, or `None` when the Guest
/// Binary has not been built outside CI. The script prints before it
/// answers, so empty stdout means it never ran.
fn eval_with_flag(critical: bool) -> Option<(Outcome, Vec<u8>)> {
    if !Path::new(WASM).exists() {
        assert!(
            std::env::var_os("CI").is_none(),
            "data/kobako.wasm missing under CI — run `bundle exec rake wasm:build`"
        );
        return None;
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
    let mut preamble = Preamble::default();
    preamble.flags = vec![Flag {
        name: "no_such_flag".into(),
        value: String::new(),
        critical,
    }];

    let snapshot = driver
        .invoke(
            Entry::Eval {
                source: b"puts 'ran'; 1 + 1",
            },
            &preamble,
            None,
        )
        .expect("the invocation must start");
    let Completion::Outcome(outcome) = snapshot.completion else {
        panic!("the invocation must answer rather than trap");
    };
    Some((outcome, snapshot.stdout.bytes))
}

// @behavior MR-017
#[test]
fn a_critical_flag_the_guest_does_not_recognise_refuses_the_invocation() {
    let Some((outcome, stdout)) = eval_with_flag(true) else {
        return;
    };
    let Outcome::Panic(panic) = outcome else {
        panic!("a critical flag the guest does not recognise must answer with a Panic");
    };

    assert_eq!(
        (panic.origin, panic.error.name.as_str(), stdout.as_slice()),
        (Origin::Sandbox, "Kobako::UnsupportedFlagError", &b""[..]),
        "a critical flag the bundled guest does not recognise through Driver#invoke must \
         refuse the invocation as the Sandbox's Kobako::UnsupportedFlagError before the \
         script runs"
    );
}

// @behavior MR-018
#[test]
fn an_advisory_flag_the_guest_does_not_recognise_is_skipped() {
    let Some(answer) = eval_with_flag(false) else {
        return;
    };

    assert_eq!(
        answer,
        (Outcome::Ok(vec![0x02]), b"ran\n".to_vec()),
        "an advisory flag the bundled guest does not recognise through Driver#invoke must be \
         skipped, leaving the invocation's value as it would be without the flag"
    );
}
