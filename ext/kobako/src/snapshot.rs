//! `Kobako::Snapshot` — the Ruby-facing per-invocation observable bundle.
//!
//! A thin magnus wrapper over the engine-neutral
//! `crate::contract::snapshot::Snapshot`: it owns one and exposes the five
//! raw readers Ruby needs. The helper methods that pack them into
//! `Kobako::Capture` (`Kobako::Snapshot#stdout` / `#stderr`) live in
//! `lib/kobako/snapshot.rb`. The split keeps the ext side a pure value
//! carrier and lets Ruby own the convenience surface. Usage is not on the
//! Snapshot — `Sandbox#usage` reads it from `Kobako::Runtime#usage`, which
//! survives the trap path where no Snapshot is produced.

use magnus::{method, prelude::*, Error as MagnusError, RModule, RString, Ruby};

use crate::contract::snapshot::Snapshot as RuntimeSnapshot;

/// Per-invocation snapshot value. Magnus wraps it so a single ext call
/// from `Runtime::eval` / `Runtime::run` returns the whole bundle — the
/// Sandbox layer decomposes it without round-tripping into ext again. The
/// inner neutral `Snapshot` is set once at construction and never mutated;
/// the five public methods registered in `init` read it out one by one.
#[magnus::wrap(class = "Kobako::Snapshot", free_immediately, size)]
pub(crate) struct Snapshot {
    inner: RuntimeSnapshot,
}

impl Snapshot {
    /// Wrap the neutral per-invocation `Snapshot` the Runtime collected
    /// once the guest export returned, the OUTCOME_BUFFER was drained, and
    /// the capture pipes were clipped to their caps.
    pub(crate) fn new(inner: RuntimeSnapshot) -> Self {
        Self { inner }
    }

    fn return_bytes(&self) -> RString {
        let ruby = Ruby::get().expect("Ruby thread");
        ruby.str_from_slice(&self.inner.return_bytes)
    }

    fn stdout_bytes(&self) -> RString {
        let ruby = Ruby::get().expect("Ruby thread");
        ruby.str_from_slice(&self.inner.stdout.bytes)
    }

    fn stdout_truncated(&self) -> bool {
        self.inner.stdout.truncated
    }

    fn stderr_bytes(&self) -> RString {
        let ruby = Ruby::get().expect("Ruby thread");
        ruby.str_from_slice(&self.inner.stderr.bytes)
    }

    fn stderr_truncated(&self) -> bool {
        self.inner.stderr.truncated
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
