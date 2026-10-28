//! Integration coverage for `Sandbox::flag` over the bundled guest, for a
//! flag it does not recognise: a critical one fails the invocation as
//! `Error::UnsupportedFlag` before the script runs, and an advisory one is
//! skipped. The script prints before it answers, so empty stdout witnesses
//! that it never ran.

// Driven through the bundled engine: these cases load a real Guest Binary,
// so they stand only in a build that carries one.
#![cfg(feature = "wasmtime")]

use crate::common::real_sandbox;
use kobako::Error;

// @behavior S-183
#[test]
fn a_critical_flag_the_guest_does_not_recognise_fails_the_invocation() {
    let Some(mut sandbox) = real_sandbox() else {
        return;
    };
    sandbox.flag("no_such_flag", "").expect("set the flag");

    let execution = sandbox.eval("puts 'ran'; 1").expect("the guest ran");

    assert!(
        matches!(execution.payload(), Err(Error::UnsupportedFlag(f)) if f.message.contains("no_such_flag"))
            && execution.stdout().is_empty(),
        "a critical flag the Guest Binary does not recognise through Sandbox::eval must fail as \
         Error::UnsupportedFlag naming it before the script runs"
    );
}

// @behavior S-184
#[test]
fn an_advisory_flag_the_guest_does_not_recognise_is_skipped() {
    let Some(mut sandbox) = real_sandbox() else {
        return;
    };
    sandbox
        .flag_advisory("no_such_flag", "8")
        .expect("set the flag");

    let execution = sandbox.eval("puts 'ran'; 1").expect("the guest ran");

    assert_eq!(
        (execution.payload().ok(), execution.stdout()),
        (Some(&[0x01][..]), &b"ran\n"[..]),
        "an advisory flag the Guest Binary does not recognise through Sandbox::eval must be \
         skipped, leaving the run as it would be without the flag"
    );
}

// @behavior S-182
#[test]
fn a_flag_name_set_twice_is_refused() {
    let Some(mut sandbox) = real_sandbox() else {
        return;
    };
    sandbox.flag("strict", "").expect("set the flag");

    assert!(
        matches!(
            sandbox.flag_advisory("strict", "on"),
            Err(Error::Argument(_))
        ),
        "a flag name set twice through Sandbox::flag must be refused as Error::Argument"
    );
}
