//! The bundled guest compiles a Preamble's source snippets on an instance
//! of their own. What comes back has to replay in a later invocation in
//! place of the source, under the same flags a replay would apply, and
//! two compiles of the same Preamble have to agree byte for byte so
//! either can stand in for the other.

mod support;

use kobako_runtime::{Completion, Entry, Runtime};
use kobako_transport::envelope::{Flag, Outcome, Preamble, Snippet, Snippets};

use support::bundled_driver as driver;

fn preamble_of(entries: Vec<Snippet>) -> Preamble {
    let mut preamble = Preamble::default();
    preamble.snippets = Snippets { entries };
    preamble
}

fn greeter() -> Preamble {
    preamble_of(vec![Snippet::Source {
        name: "Greeter".into(),
        body: "module Greeter; def self.hi = 'hi'; end".into(),
    }])
}

// @behavior MR-024
#[test]
fn compiled_snippets_replay_in_a_later_invocation() {
    let Some(driver) = driver() else {
        return;
    };
    let bytecodes = driver
        .compile(&greeter())
        .expect("a source snippet that parses must compile");
    let replay = preamble_of(
        bytecodes
            .into_iter()
            .map(|body| Snippet::Bytecode { body })
            .collect(),
    );

    let snapshot = driver
        .invoke(
            Entry::Eval {
                source: b"Greeter.hi",
            },
            &replay,
            None,
        )
        .expect("the invocation must start");

    assert!(
        matches!(snapshot.completion, Completion::Outcome(Outcome::Ok(ref value)) if value == b"\xa2hi"),
        "bytecode from Driver#compile, sent back as bytecode snippets, must define what the \
         source did for a later invocation"
    );
}

// @behavior MR-023
#[test]
fn compiling_the_same_preamble_twice_yields_the_same_bytes() {
    let Some(driver) = driver() else {
        return;
    };

    assert_eq!(
        driver.compile(&greeter()),
        driver.compile(&greeter()),
        "the same Preamble through Driver#compile on two fresh instances must answer \
         byte-identical bytecode"
    );
}

// @behavior MR-025
#[test]
fn a_critical_flag_the_guest_does_not_recognise_leaves_nothing_compiled() {
    let Some(driver) = driver() else {
        return;
    };
    let mut preamble = greeter();
    preamble.flags = vec![Flag {
        name: "no_such_flag".into(),
        value: String::new(),
        critical: true,
    }];

    assert_eq!(
        driver.compile(&preamble),
        None,
        "a critical flag the bundled guest does not recognise through Driver#compile must \
         leave nothing compiled, since compiling applies the flags a replay would"
    );
}
