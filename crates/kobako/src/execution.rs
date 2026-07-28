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
//! Execution off the exception; the SDK keeps the failure a value on
//! `outcome`, and `into_value` folds it back into a `Result` so a caller
//! that only wants the value cannot silently pass over a guest failure —
//! the footgun `std::process::Output`'s ignorable `status` is known for.
//! And where Ruby eagerly restores a result Handle to its host object
//! during decode, the SDK resolves lazily through `resolve`, so the wire
//! `Value::Handle` stays inspectable and the host object is recovered on
//! demand.

use std::fmt;
use std::sync::{Arc, Mutex};

#[cfg(feature = "msgpack")]
use kobako_codec::msgpack::codec::Value;
use kobako_runtime::snapshot::{Capture, Usage};

use crate::error::Error;
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

    /// The outcome decoded through the default payload codec, with every
    /// Handle in it checked live. A guest cannot fabricate a Handle, so
    /// an unknown id means a corrupted runtime and fails like a malformed
    /// value.
    ///
    /// The decode happens here rather than at invocation because it is
    /// the one step that needs a schema; a host with its own reads
    /// `payload` instead. It runs per call, so a caller reading the value
    /// more than once holds onto what it gets.
    #[cfg(feature = "msgpack")]
    pub fn value(&self) -> Result<Value, Error> {
        let bytes = self.outcome.as_ref().map_err(Clone::clone)?;
        let value = crate::outcome::decode_value(bytes)?;
        self.require_live_handles(&value)?;
        Ok(value)
    }

    /// Consume the Execution and fold its outcome into a `Result` — the
    /// ergonomic path for a caller that wants the value and lets a guest
    /// failure propagate with `?`. Reach for the captures / `usage`
    /// before calling this, since it drops them.
    #[cfg(feature = "msgpack")]
    pub fn into_value(self) -> Result<Value, Error> {
        self.value()
    }

    #[cfg(feature = "msgpack")]
    fn require_live_handles(&self, value: &Value) -> Result<(), Error> {
        match value {
            Value::Handle(id) => self.resolve(*id).map(|_| ()).ok_or_else(|| {
                Error::Sandbox(Box::new(crate::error::Failure {
                    class: "Kobako::SandboxError".into(),
                    message: format!("unknown Handle id: {id}"),
                    backtrace: Vec::new(),
                    available: Vec::new(),
                    diagnostic: None,
                }))
            }),
            Value::Array(items) => items.iter().try_for_each(|v| self.require_live_handles(v)),
            Value::Map(pairs) => pairs.iter().try_for_each(|(key, val)| {
                self.require_live_handles(key)?;
                self.require_live_handles(val)
            }),
            _ => Ok(()),
        }
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

    /// The bundled codec's spelling: resolve a `Value::Handle`, and
    /// nothing else, through `resolve`.
    #[cfg(feature = "msgpack")]
    pub fn resolve_value(&self, value: &Value) -> Option<Arc<dyn Receiver>> {
        Handles::new(&self.handles).resolve_value(value)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::receiver::Fault;
    use crate::yielder::Yielder;

    /// A byte-level receiver — the shape a host with its own schema
    /// writes, so the table these tests resolve against holds no
    /// codec-flavoured wrapper.
    struct Probe;

    impl Receiver for Probe {
        fn call(
            &self,
            _method: &str,
            _payload: &[u8],
            _block: Option<&mut Yielder<'_>>,
            _handles: &Handles<'_>,
        ) -> Result<Vec<u8>, Fault> {
            Ok(Vec::new())
        }
    }

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
