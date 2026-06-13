//! `Kobako::Snapshot` — per-invocation observable bundle.
//!
//! Every successful `Kobako::Runtime#eval` / `#run` returns one of these.
//! It carries every observable the host needs to surface after a guest
//! invocation: the OUTCOME_BUFFER bytes (`return_bytes`), the captured
//! stdout / stderr byte slices with their truncation flags, and
//! the wall-clock + memory-peak figures from `Kobako::Usage`.
//!
//! Ruby callers see the seven raw readers registered below; the helper
//! methods that pack them into `Kobako::Capture` / `Kobako::Usage`
//! (`Kobako::Snapshot#stdout` / `#stderr` / `#usage`) live in
//! `lib/kobako/snapshot.rb`. The split keeps the ext side a pure value
//! carrier and lets Ruby own the convenience surface.

use std::cell::Cell;
use std::time::Duration;

use magnus::{method, prelude::*, Error as MagnusError, RModule, RString, Ruby};

/// Per-invocation snapshot value. Magnus wraps it so a single ext call
/// from `Runtime::eval` / `Runtime::run` returns the whole bundle —
/// the Sandbox layer can decompose it without round-tripping into ext
/// again. All fields are private; the seven public methods registered
/// in `init` read them out one by one. The wall-clock duration is
/// held as a `Cell<Duration>` only because magnus' `#[magnus::wrap]`
/// macro requires interior mutability — every field is set once at
/// construction time and never mutated again.
#[magnus::wrap(class = "Kobako::Snapshot", free_immediately, size)]
pub(crate) struct Snapshot {
    return_bytes: Vec<u8>,
    stdout_bytes: Vec<u8>,
    stdout_truncated: bool,
    stderr_bytes: Vec<u8>,
    stderr_truncated: bool,
    wall_time: Cell<Duration>,
    memory_peak: usize,
}

impl Snapshot {
    /// Construct a fresh Snapshot from the per-invocation data the
    /// Runtime has just collected. Called from
    /// `crate::runtime::Runtime::build_snapshot` once the
    /// guest export has returned, the OUTCOME_BUFFER has been drained,
    /// and the capture pipes have been clipped to their caps.
    pub(crate) fn new(
        return_bytes: Vec<u8>,
        stdout_bytes: Vec<u8>,
        stdout_truncated: bool,
        stderr_bytes: Vec<u8>,
        stderr_truncated: bool,
        wall_time: Duration,
        memory_peak: usize,
    ) -> Self {
        Self {
            return_bytes,
            stdout_bytes,
            stdout_truncated,
            stderr_bytes,
            stderr_truncated,
            wall_time: Cell::new(wall_time),
            memory_peak,
        }
    }

    fn return_bytes(&self) -> RString {
        let ruby = Ruby::get().expect("Ruby thread");
        ruby.str_from_slice(&self.return_bytes)
    }

    fn stdout_bytes(&self) -> RString {
        let ruby = Ruby::get().expect("Ruby thread");
        ruby.str_from_slice(&self.stdout_bytes)
    }

    fn stdout_truncated(&self) -> bool {
        self.stdout_truncated
    }

    fn stderr_bytes(&self) -> RString {
        let ruby = Ruby::get().expect("Ruby thread");
        ruby.str_from_slice(&self.stderr_bytes)
    }

    fn stderr_truncated(&self) -> bool {
        self.stderr_truncated
    }

    fn wall_time(&self) -> f64 {
        self.wall_time.get().as_secs_f64()
    }

    fn memory_peak(&self) -> usize {
        self.memory_peak
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
