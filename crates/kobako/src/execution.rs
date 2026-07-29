//! `Execution` — the frozen record of one Sandbox invocation.
//!
//! The Rust counterpart of the Ruby gem's `Kobako::Execution`: any
//! invocation that reached the guest produces one, carrying the run's
//! output captures, resource `usage`, and the guest-level `outcome` —
//! the decoded value, or the taxonomy `Error` a guest failure or trap
//! attributes to. `eval` / `run` return it whenever a run started; a run
//! that never started (setup, seal, or a host pre-flight refusal) is the
//! outer `Err` and has no Execution.
//!
//! Two divergences from the Ruby frontend are deliberate, not
//! oversights. Ruby raises on guest failure and hangs the same frozen
//! Execution off the exception; the SDK keeps the failure a value, and
//! `payload` / `value` return a `Result` so a caller that only wants the
//! value cannot silently pass over a guest failure — the footgun
//! `std::process::Output`'s ignorable `status` is known for.
//! And where Ruby eagerly restores a result Handle to its host object
//! during decode, the SDK resolves lazily through `resolve`, so the wire
//! `Value::Handle` stays inspectable and the host object is recovered on
//! demand.

use std::fmt;
use std::sync::{Arc, Mutex};

use kobako_runtime::snapshot::Capture;
// The observables the contract already expresses frontend-free: `usage`
// returns this type, so it is re-exported from here rather than restated.
pub use kobako_runtime::snapshot::Usage;
use kobako_transport::envelope::{Origin, Outcome, Panic};

use crate::error::{Error, Failure};
use crate::handles::{HandleTable, Handles};
use crate::receiver::Receiver;

/// The frozen result of one `eval` / `run`: the guest-level `outcome`
/// plus the run's captures, `usage`, and the Handle table its result
/// resolves against. Owned by the caller and independent of the
/// `Sandbox`, so it survives concurrent invocations unchanged.
pub struct Execution {
    outcome: Result<Vec<u8>, Error>,
    // Reached through `resolve`, which takes the id the outcome carried:
    // finding that id is the schema's job, reaching the object is not.
    handles: Arc<Mutex<HandleTable>>,
    stdout: Capture,
    stderr: Capture,
    usage: Usage,
}

impl Execution {
    /// Assemble the record from a cooked outcome and the invocation's
    /// observables. The `Sandbox` owns the raw-`Snapshot`-to-outcome
    /// cook (decode, Handle liveness); this is the plain data holder.
    pub(crate) fn new(
        outcome: Result<Vec<u8>, Error>,
        handles: Arc<Mutex<HandleTable>>,
        stdout: Capture,
        stderr: Capture,
        usage: Usage,
    ) -> Self {
        Execution {
            outcome,
            handles,
            stdout,
            stderr,
            usage,
        }
    }

    /// The guest-level outcome as the wire carried it: `Ok` is the
    /// Result arm's payload bytes, `Err` the taxonomy attribution of a
    /// guest failure or trap. The captures and `usage` stay readable on
    /// either arm.
    ///
    /// Bytes because the payload's schema is the host's own — attributing
    /// an invocation is the envelope's job and reads no payload byte, so
    /// a host whose Receivers speak another schema reads its own result
    /// here and decodes it itself.
    pub fn payload(&self) -> Result<&[u8], &Error> {
        self.outcome.as_ref().map(Vec::as_slice)
    }

    /// Resolve a Handle id this run's result carried to the live host
    /// object it stands for; `None` for an id the invocation never
    /// issued. The table lives as long as the Execution, so a resolved
    /// object outlives the invocation that produced it. Upcast the `Arc`
    /// to `Arc<dyn Any + Send + Sync>` and `downcast` to recover the
    /// concrete receiver type.
    ///
    /// An id rather than a decoded value, mirroring `Handles::resolve`:
    /// finding a Handle inside a result is the schema's job, so a host
    /// reading its own outcome bytes reaches the object from here without
    /// a codec.
    pub fn resolve(&self, id: u32) -> Option<Arc<dyn Receiver>> {
        Handles::new(&self.handles).resolve(id)
    }

    /// Bytes the guest wrote to `$stdout` during this run.
    pub fn stdout(&self) -> &[u8] {
        &self.stdout.bytes
    }

    /// Bytes the guest wrote to `$stderr` during this run.
    pub fn stderr(&self) -> &[u8] {
        &self.stderr.bytes
    }

    /// Whether the stdout cap clipped this run's output.
    pub fn stdout_truncated(&self) -> bool {
        self.stdout.truncated
    }

    /// Whether the stderr cap clipped this run's output.
    pub fn stderr_truncated(&self) -> bool {
        self.stderr.truncated
    }

    /// Resource usage of this run.
    pub fn usage(&self) -> Usage {
        self.usage
    }
}

impl fmt::Debug for Execution {
    /// The outcome and capture sizes — enough to see what a run produced
    /// without dumping capture bytes or the opaque Handle table.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Execution")
            .field("outcome", &self.outcome)
            .field("stdout_len", &self.stdout.bytes.len())
            .field("stderr_len", &self.stderr.bytes.len())
            .field("wall_time", &self.usage.wall_time)
            .field("memory_peak", &self.usage.memory_peak)
            .finish()
    }
}

/// Classify one OUTCOME_BUFFER by its envelope alone: the Result arm's
/// payload bytes, or the `Error` its failure attributes to. Reads no
/// payload byte, so attribution works for a host whose Receivers speak
/// a schema this crate does not know.
///
/// One half of turning a raw `Snapshot` into the `Execution` above — the
/// half that reads the envelope; `Sandbox`'s `build_execution` is the
/// other. The Ruby gem's `Kobako::Outcome` is its twin, so both frontends
/// reading the same bytes reach the same variant.
pub(crate) fn classify(bytes: &[u8]) -> Result<Vec<u8>, Error> {
    match Outcome::decode(bytes) {
        Ok(Outcome::Result(payload)) => Ok(payload),
        Ok(Outcome::Panic(panic)) => Err(classify_panic(panic)),
        // Framing the outcome is the one thing the host does before
        // attribution, so a message it cannot frame — an empty buffer
        // included — leaves nothing to attribute to.
        Err(_) if bytes.is_empty() => Err(Error::Trap(
            "Sandbox exited without producing a result".into(),
        )),
        Err(_) => Err(Error::Trap(
            "Sandbox produced an unrecognised result; the runtime is corrupted, \
             discard this Sandbox before another invocation"
                .into(),
        )),
    }
}

/// `origin == "service"` → `Service`; a sandbox-origin panic carrying
/// the bytecode rejection class → `Bytecode`; everything else →
/// `Sandbox`. Every field is typed at the envelope, so classifying a
/// Panic reads no payload byte and cannot fail.
fn classify_panic(panic: Panic) -> Error {
    let from_service = panic.origin == Origin::Service;
    let failure = Box::new(Failure {
        name: panic.error.name,
        message: panic.error.message,
        backtrace: panic.error.backtrace,
        available: panic.available,
        diagnostic: None,
    });
    if from_service {
        Error::Service(failure)
    } else if failure.name == "Kobako::BytecodeError" {
        Error::Bytecode(failure)
    } else {
        Error::Sandbox(failure)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::receiver::Probe;

    /// An Execution whose table already holds `object`, paired with the
    /// id that invocation issued for it.
    fn execution_holding(object: Arc<dyn Receiver>) -> (Execution, u32) {
        let table = Arc::new(Mutex::new(HandleTable::default()));
        let id = table
            .lock()
            .expect("a fresh table mutex is never poisoned")
            .alloc(object)
            .expect("the first id is far below the cap");
        let execution = Execution::new(
            Ok(Vec::new()),
            table,
            Capture::default(),
            Capture::default(),
            Usage::default(),
        );
        (execution, id)
    }

    #[test]
    fn resolve_recovers_the_object_the_outcome_handle_id_stands_for() {
        let object: Arc<dyn Receiver> = Arc::new(Probe);
        let (execution, id) = execution_holding(object.clone());

        let resolved = execution.resolve(id).expect("the invocation issued id");

        assert!(
            Arc::ptr_eq(&resolved, &object),
            "a Handle id read out of an outcome through Execution::resolve must \
             yield the very object the invocation bound to it"
        );
    }

    #[test]
    fn resolve_refuses_an_id_the_invocation_never_issued() {
        let (execution, id) = execution_holding(Arc::new(Probe));

        assert!(
            execution.resolve(id + 1).is_none(),
            "an unissued id through Execution::resolve must resolve to nothing, \
             so a corrupted outcome reaches no host object"
        );
    }
}

#[cfg(test)]
mod classify_tests {
    use kobako_transport::envelope::ErrorRecord;

    use super::*;

    fn panic_bytes(origin: Origin, name: &str) -> Vec<u8> {
        Outcome::Panic(Panic {
            origin,
            error: ErrorRecord {
                name: name.into(),
                message: "boom".into(),
                backtrace: Vec::new(),
            },
            available: Vec::new(),
        })
        .encode()
    }

    #[test]
    fn the_result_arm_yields_the_payload_bytes_it_carried() {
        assert_eq!(
            classify(&Outcome::Result(vec![0x2a]).encode()).unwrap(),
            vec![0x2a],
            "a Result arm through classify must hand back its payload bytes untouched, \
             since attribution reads no payload byte"
        );
    }

    #[test]
    fn service_origin_panic_becomes_service_error() {
        let result = classify(&panic_bytes(Origin::Service, "Kobako::ServiceError"));
        assert!(matches!(result, Err(Error::Service(f)) if f.message == "boom"));
    }

    #[test]
    fn bytecode_class_panic_becomes_bytecode_error() {
        let result = classify(&panic_bytes(Origin::Sandbox, "Kobako::BytecodeError"));
        assert!(matches!(result, Err(Error::Bytecode(_))));
    }

    #[test]
    fn sandbox_origin_panic_becomes_sandbox_error() {
        let result = classify(&panic_bytes(Origin::Sandbox, "RuntimeError"));
        assert!(matches!(result, Err(Error::Sandbox(f)) if f.name == "RuntimeError"));
    }

    #[test]
    fn empty_bytes_walk_the_trap_path() {
        assert!(matches!(classify(&[]), Err(Error::Trap(_))));
    }

    #[test]
    fn unknown_tag_walks_the_trap_path() {
        assert!(matches!(classify(&[0x7f, 0x2a]), Err(Error::Trap(_))));
    }

    #[test]
    fn a_panic_record_the_envelope_cannot_frame_walks_the_trap_path() {
        // The Panic arm followed by a truncated origin length prefix.
        let result = classify(&[0x02, 0x00, 0x00]);
        assert!(
            matches!(result, Err(Error::Trap(_))),
            "a Panic the envelope cannot frame leaves nothing to attribute to, got {result:?}"
        );
    }

    // E-27: an unresolved entrypoint reaches the caller with the names it
    // could have been, matching what the Ruby frontend exposes as
    // `#available` on its own subclass.
    #[test]
    fn an_unresolved_entrypoint_carries_the_names_it_could_have_been() {
        let bytes = Outcome::Panic(Panic {
            origin: Origin::Sandbox,
            error: ErrorRecord {
                name: "Kobako::UndefinedEntrypointError".into(),
                message: "undefined entrypoint: Wrker".into(),
                backtrace: Vec::new(),
            },
            available: vec!["Worker".into(), "Helper".into()],
        })
        .encode();
        let result = classify(&bytes);
        assert!(
            matches!(result, Err(Error::Sandbox(ref f)) if f.available == ["Worker", "Helper"]),
            "an unresolved entrypoint must reach the caller with its correction, got {result:?}"
        );
    }
}
