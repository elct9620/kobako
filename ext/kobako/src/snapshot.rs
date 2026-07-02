//! `Kobako::Snapshot` — the Ruby-facing per-invocation observable bundle.
//!
//! The success-path view of the engine-neutral snapshot: the outcome bytes
//! and the two captured output channels, exposed through five raw readers.
//! The helper methods that pack them into `Kobako::Capture`
//! (`Kobako::Snapshot#stdout` / `#stderr`) live in
//! `lib/kobako/snapshot.rb`. The split keeps the ext side a pure value
//! carrier and lets Ruby own the convenience surface. Usage is not on the
//! Snapshot — `Sandbox#usage` reads it from `Kobako::Runtime#usage`, which
//! survives the trap path where no `Kobako::Snapshot` is produced.

use magnus::{method, prelude::*, Error as MagnusError, RModule, RString, Ruby};

use crate::contract::snapshot::Capture;

/// Per-invocation snapshot value. Magnus wraps it so a single ext call
/// from `Runtime::eval` / `Runtime::run` returns the whole bundle — the
/// Sandbox layer decomposes it without round-tripping into ext again. The
/// fields are set once at construction and never mutated; the five public
/// methods registered in `init` read them out one by one.
#[magnus::wrap(class = "Kobako::Snapshot", free_immediately, size)]
pub(crate) struct Snapshot {
    return_bytes: Vec<u8>,
    stdout: Capture,
    stderr: Capture,
}

impl Snapshot {
    /// Bundle the success outputs the Runtime collected once the guest
    /// export returned with an outcome: the drained OUTCOME_BUFFER bytes
    /// and the capture pipes clipped to their caps.
    pub(crate) fn new(return_bytes: Vec<u8>, stdout: Capture, stderr: Capture) -> Self {
        Self {
            return_bytes,
            stdout,
            stderr,
        }
    }

    fn return_bytes(&self) -> RString {
        let ruby = Ruby::get().expect("Ruby thread");
        ruby.str_from_slice(&self.return_bytes)
    }

    fn stdout_bytes(&self) -> RString {
        let ruby = Ruby::get().expect("Ruby thread");
        ruby.str_from_slice(&self.stdout.bytes)
    }

    fn stdout_truncated(&self) -> bool {
        self.stdout.truncated
    }

    fn stderr_bytes(&self) -> RString {
        let ruby = Ruby::get().expect("Ruby thread");
        ruby.str_from_slice(&self.stderr.bytes)
    }

    fn stderr_truncated(&self) -> bool {
        self.stderr.truncated
    }
}

/// Register `Kobako::Snapshot` plus its five raw readers under the
/// `Kobako` module. Called from `crate::init` after `Kobako::Runtime`
/// is registered so the magnus wrap macro can resolve the class name.
pub(crate) fn init(ruby: &Ruby, kobako: RModule) -> Result<(), MagnusError> {
    let snapshot = kobako.define_class("Snapshot", ruby.class_object())?;
    snapshot.define_method("return_bytes", method!(Snapshot::return_bytes, 0))?;
    snapshot.define_method("stdout_bytes", method!(Snapshot::stdout_bytes, 0))?;
    snapshot.define_method("stdout_truncated", method!(Snapshot::stdout_truncated, 0))?;
    snapshot.define_method("stderr_bytes", method!(Snapshot::stderr_bytes, 0))?;
    snapshot.define_method("stderr_truncated", method!(Snapshot::stderr_truncated, 0))?;
    Ok(())
}
