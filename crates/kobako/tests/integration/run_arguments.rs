//! What a value-tree `run` carries in: a capability reference the Host App
//! wrote is refused before the guest runs, while a host object handed over
//! still crosses as one.
#![cfg(all(feature = "msgpack", feature = "wasmtime"))]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::common::real_sandbox;
use kobako::msgpack::{RunArg, Value, ValueReceiver};
use kobako::{Error, Fault, Handles, RunPayload, Sandbox, Yielder};

/// A host object that answers `"reached"` and records that the guest
/// called it.
#[derive(Clone, Default)]
struct Probe(Arc<AtomicBool>);

impl Probe {
    fn reached(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

impl ValueReceiver for Probe {
    fn call(
        &self,
        _method: &str,
        _args: &[Value],
        _kwargs: &[(String, Value)],
        _block: Option<&mut Yielder<'_>>,
        _handles: &Handles<'_>,
    ) -> Result<Value, Fault> {
        self.0.store(true, Ordering::SeqCst);
        Ok(Value::Str("reached".into()))
    }
}

/// A Sandbox whose `Worker` entrypoint calls the bound `Probe` first, so a
/// run the guest began leaves the probe reached.
fn sandbox_reaching(probe: &Probe) -> Option<Sandbox> {
    let mut sandbox = real_sandbox()?;
    sandbox
        .bind("Probe", probe.clone().into_receiver())
        .expect("bind the probe");
    sandbox
        .preload("Worker", "Worker = ->(*a, **_k) { Probe.call; a.first }")
        .expect("preload the entrypoint");
    Some(sandbox)
}

// @behavior T-389
#[test]
fn a_run_argument_carrying_a_written_handle_fails_before_the_guest_runs() {
    let written = [
        Value::Handle(1),
        Value::Array(vec![Value::Int(1), Value::Handle(1)]),
        Value::Map(vec![(Value::Str("k".into()), Value::Handle(1))]),
    ];
    for value in written {
        let probe = Probe::default();
        let Some(sandbox) = sandbox_reaching(&probe) else {
            return;
        };
        let err = sandbox
            .run(
                "Worker",
                RunPayload::from_values(vec![RunArg::Value(value.clone())], vec![]),
            )
            .expect_err("a run argument carrying a Handle the Host App wrote must fail");

        assert!(
            matches!(err, Error::Argument(_)),
            "a Handle written into run's arguments as {value:?} must be the Host App's \
             argument error, got {err:?}"
        );
        assert!(
            !probe.reached(),
            "a run refused for a written Handle in {value:?} must fail before the guest runs"
        );
    }
}

// @behavior T-389
#[test]
fn a_host_object_handed_to_run_still_crosses_as_a_capability_reference() {
    let probe = Probe::default();
    let Some(mut sandbox) = real_sandbox() else {
        return;
    };
    sandbox
        .preload("Worker", "Worker = ->(store) { store.get }")
        .expect("preload the entrypoint");

    let value = sandbox
        .run(
            "Worker",
            RunPayload::from_values(vec![RunArg::Object(probe.into_receiver())], vec![]),
        )
        .expect("a host object handed to run must reach the guest")
        .value()
        .expect("the guest returns what the host object answered");

    assert_eq!(
        value,
        Value::Str("reached".into()),
        "a host object handed to run must cross as a reference the guest can call"
    );
}
