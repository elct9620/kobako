//! Integration coverage for the engine seam: a Sandbox driven by a
//! runtime the caller supplied rather than the bundled wasmtime one.
//!
//! `kobako-runtime` is where the contract lives, and the point of these
//! tests is that satisfying it is enough — nothing above the contract
//! names an engine type, so a host that brings its own engine keeps the
//! whole SDK (Catalog, Handles, snippet replay) unchanged.

use std::sync::{Arc, Mutex};

use kobako_runtime::dispatch::DispatchHandler;
use kobako_runtime::runtime::{Entry, Frames, Runtime};
use kobako_runtime::snapshot::{Capture, Completion, Snapshot, Usage};
use kobako_transport::envelope::Outcome;

use kobako::{Error, Options, Profile, RunPayload, Sandbox};

/// A runtime that runs no wasm: it answers every invocation with one
/// canned outcome and records the frames it was driven with.
struct Canned {
    payload: Vec<u8>,
    profile: Profile,
    preambles: Arc<Mutex<Vec<Vec<u8>>>>,
}

impl Canned {
    fn new(payload: Vec<u8>, profile: Profile) -> Self {
        Canned {
            payload,
            profile,
            preambles: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl Runtime for Canned {
    fn invoke(
        &self,
        _entry: Entry<'_>,
        frames: Frames<'_>,
        _handler: Option<Arc<dyn DispatchHandler>>,
    ) -> Result<Snapshot, kobako_runtime::error::InvokeError> {
        self.preambles
            .lock()
            .unwrap()
            .push(frames.preamble.to_vec());
        Ok(Snapshot {
            completion: Completion::Outcome(Outcome::Ok(self.payload.clone()).encode()),
            stdout: Capture::default(),
            stderr: Capture::default(),
            usage: Usage::default(),
        })
    }

    fn profile(&self) -> Profile {
        self.profile
    }
}

#[test]
fn an_injected_runtime_drives_an_invocation_the_sandbox_set_up() {
    let engine = Canned::new(vec![0x2a], Profile::Hermetic);
    let preambles = engine.preambles.clone();
    let mut sandbox = Sandbox::with_runtime(engine, Profile::Hermetic)
        .expect("a hermetic engine meets the floor");
    sandbox.bind_fillable("MyService::KV").unwrap();

    let execution = sandbox.run("App", RunPayload::bytes(vec![0x01])).unwrap();

    assert_eq!(
        (
            execution.payload().unwrap(),
            preambles.lock().unwrap().len()
        ),
        (&[0x2a][..], 1),
        "a Sandbox over a caller's engine must drive that engine once per invocation \
         and hand back the outcome it produced"
    );
}

#[test]
fn an_injected_runtime_carries_the_registrations_the_sandbox_sealed() {
    let engine = Canned::new(Vec::new(), Profile::Hermetic);
    let preambles = engine.preambles.clone();
    let mut sandbox = Sandbox::with_runtime(engine, Profile::Hermetic).unwrap();
    sandbox.bind_fillable("MyService::KV").unwrap();

    sandbox.eval("1").unwrap();

    assert!(
        !preambles.lock().unwrap()[0].is_empty(),
        "an engine behind the contract must be handed the sealed catalog's Frame 1, \
         since registration is the Sandbox's and never the engine's"
    );
}

#[test]
fn a_runtime_declaring_less_than_the_requested_floor_is_refused() {
    let engine = Canned::new(Vec::new(), Profile::Permissive);

    let refusal = Sandbox::with_runtime(engine, Profile::Hermetic);

    assert!(
        matches!(refusal, Err(Error::Setup(_))),
        "an engine declaring a posture below the requested floor must fail construction, \
         so a host never runs untrusted code under less isolation than it asked for"
    );
}

#[test]
fn a_runtime_declaring_more_than_the_requested_floor_is_accepted() {
    let engine = Canned::new(Vec::new(), Profile::Hermetic);

    assert!(
        Sandbox::with_runtime(engine, Profile::Permissive).is_ok(),
        "the request is a floor, not an equality, so an engine stricter than asked for \
         must still construct"
    );
}

#[test]
fn the_default_options_floor_reaches_the_bundled_driver_unchanged() {
    assert_eq!(
        Options::default().profile,
        Profile::Hermetic,
        "a Sandbox built without stating a profile must ask for the hermetic floor, \
         so the secure posture is the one a caller gets by saying nothing"
    );
}
