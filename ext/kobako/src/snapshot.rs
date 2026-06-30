//! `Kobako::Snapshot` — the Ruby-facing per-invocation observable bundle.
//!
//! A thin magnus wrapper over the engine-neutral
//! `crate::contract::snapshot::Snapshot`: it owns one and exposes the seven
//! raw readers Ruby needs. The helper methods that pack them into
//! `Kobako::Capture` / `Kobako::Usage` (`Kobako::Snapshot#stdout` /
//! `#stderr` / `#usage`) live in `lib/kobako/snapshot.rb`. The split keeps
//! the ext side a pure value carrier and lets Ruby own the convenience
//! surface.

use magnus::{method, prelude::*, Error as MagnusError, RModule, RString, Ruby};

use crate::contract::snapshot::Snapshot as RuntimeSnapshot;

/// Per-invocation snapshot value. Magnus wraps it so a single ext call
/// from `Runtime::eval` / `Runtime::run` returns the whole bundle — the
/// Sandbox layer decomposes it without round-tripping into ext again. The
/// inner neutral `Snapshot` is set once at construction and never mutated;
/// the seven public methods registered in `init` read it out one by one.
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

    fn wall_time(&self) -> f64 {
        self.inner.usage.wall_time.as_secs_f64()
    }

    fn memory_peak(&self) -> usize {
        self.inner.usage.memory_peak
    }
}

/// Register `Kobako::Snapshot` plus its seven raw readers under the
/// `Kobako` module. Called from `crate::init` after `Kobako::Runtime`
/// is registered so the magnus wrap macro can resolve the class name.
pub(crate) fn init(ruby: &Ruby, kobako: RModule) -> Result<(), MagnusError> {
    let snapshot = kobako.define_class("Snapshot", ruby.class_object())?;
    snapshot.define_method("return_bytes", method!(Snapshot::return_bytes, 0))?;
    snapshot.define_method("stdout_bytes", method!(Snapshot::stdout_bytes, 0))?;
    snapshot.define_method("stdout_truncated", method!(Snapshot::stdout_truncated, 0))?;
    snapshot.define_method("stderr_bytes", method!(Snapshot::stderr_bytes, 0))?;
    snapshot.define_method("stderr_truncated", method!(Snapshot::stderr_truncated, 0))?;
    snapshot.define_method("wall_time", method!(Snapshot::wall_time, 0))?;
    snapshot.define_method("memory_peak", method!(Snapshot::memory_peak, 0))?;
    Ok(())
}
