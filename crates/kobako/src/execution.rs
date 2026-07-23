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

use kobako_codec::codec::Value;
use kobako_runtime::snapshot::{Capture, Usage};

use crate::error::Error;
use crate::handles::{HandleTable, Handles};
use crate::receiver::Receiver;

/// The frozen result of one `eval` / `run`: the guest-level `outcome`
/// plus the run's captures, `usage`, and the Handle table its result
/// resolves against. Owned by the caller and independent of the
/// `Sandbox`, so it survives concurrent invocations unchanged.
pub struct Execution {
    outcome: Result<Value, Error>,
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
        outcome: Result<Value, Error>,
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

    /// The guest-level outcome by reference: `Ok` is the decoded value,
    /// `Err` the taxonomy attribution of a guest failure or trap. The
    /// captures and `usage` stay readable on either arm.
    pub fn value(&self) -> Result<&Value, &Error> {
        self.outcome.as_ref()
    }

    /// Consume the Execution and fold its outcome into a `Result` — the
    /// ergonomic path for a caller that wants the value and lets a guest
    /// failure propagate with `?`. Reach for the captures / `usage`
    /// before calling this, since it drops them.
    pub fn into_value(self) -> Result<Value, Error> {
        self.outcome
    }

    /// Resolve a `Value::Handle` from this run's result to the live host
    /// object it stands for; `None` for a non-Handle value. The table
    /// lives as long as the Execution, so a resolved object outlives the
    /// invocation that produced it. Upcast the `Arc` to
    /// `Arc<dyn Any + Send + Sync>` and `downcast` to recover the
    /// concrete receiver type.
    pub fn resolve(&self, value: &Value) -> Option<Arc<dyn Receiver>> {
        Handles::new(&self.handles).resolve(value)
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
