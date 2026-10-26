//! `Kobako::Snapshot`: one invocation's result at the Ruby boundary.

use magnus::{
    method, prelude::*, typed_data::DataTypeFunctions, Error as MagnusError, Exception, RModule,
    RString, Ruby, TypedData,
};

use kobako_runtime::snapshot::{Completion, Snapshot as RuntimeSnapshot};
use kobako_transport::envelope::Outcome;

use super::errors;

/// A Panic's fields as they cross to Ruby: origin, class, message,
/// backtrace, and the names the invocation could have used in place of the
/// one it named. Named so the Ruby side reads them positionally against one
/// shape, matching the `Kobako::Outcome::panic_fields` alias.
type PanicFields = (String, String, String, Vec<String>, Vec<String>);

pub(super) fn register(ruby: &Ruby, kobako: RModule) -> Result<(), MagnusError> {
    let snapshot = kobako.define_class("Snapshot", ruby.class_object())?;
    snapshot.define_method("outcome", method!(Snapshot::outcome, 0))?;
    snapshot.define_method("trap_error", method!(Snapshot::trap_error, 0))?;
    snapshot.define_method("wall_time", method!(Snapshot::wall_time, 0))?;
    snapshot.define_method("memory_peak", method!(Snapshot::memory_peak, 0))?;
    snapshot.define_method("stdout", method!(Snapshot::stdout, 0))?;
    snapshot.define_method("stdout_truncated?", method!(Snapshot::stdout_truncated, 0))?;
    snapshot.define_method("stderr", method!(Snapshot::stderr, 0))?;
    snapshot.define_method("stderr_truncated?", method!(Snapshot::stderr_truncated, 0))?;
    Ok(())
}

/// One invocation's result at the Ruby boundary — the whole `Snapshot` the
/// driver produced, exposed as `Kobako::Snapshot`. Usage and the
/// two output captures are present on every outcome, so the trap path
/// carries them just like the value path; the completion is read as either
/// the Outcome (`#outcome`) or a trap (`#trap_error`). The verb names the
/// invocation, so an unsupported one says which operation is lacking.
#[derive(TypedData)]
#[magnus(class = "Kobako::Snapshot", free_immediately, size)]
pub(super) struct Snapshot(RuntimeSnapshot, &'static str);

impl DataTypeFunctions for Snapshot {}

impl Snapshot {
    pub(super) fn new(snapshot: RuntimeSnapshot, verb: &'static str) -> Self {
        Self(snapshot, verb)
    }

    /// Already split off the core envelope, so the Ruby side maps a failure
    /// onto its error taxonomy without decoding a payload byte: the ok
    /// arm's payload, or a Panic's fields beside an empty payload. A trap
    /// has no Outcome; `#trap_error` is the discriminator there.
    fn outcome(&self) -> Result<(RString, Option<PanicFields>), MagnusError> {
        let ruby = Ruby::get().expect("Ruby thread");
        match &self.0.completion {
            Completion::Outcome(Outcome::Ok(value)) => Ok((ruby.str_from_slice(value), None)),
            Completion::Outcome(Outcome::Panic(panic)) => Ok((
                ruby.str_from_slice(&[]),
                Some((
                    panic.origin.name().to_owned(),
                    panic.error.name.clone(),
                    panic.error.message.clone(),
                    panic.error.backtrace.clone(),
                    panic.available.clone(),
                )),
            )),
            // The Guest Binary answered rather than broke, so it reads as
            // the Sandbox's failure, worded by the host.
            Completion::Outcome(Outcome::Unsupported(_)) => Ok((
                ruby.str_from_slice(&[]),
                Some((
                    "sandbox".to_owned(),
                    "Kobako::SandboxError".to_owned(),
                    format!(
                        "the Sandbox runtime does not support #{}; \
                         rebuild it against the kobako version in use",
                        self.1
                    ),
                    Vec::new(),
                    Vec::new(),
                )),
            )),
            Completion::Trap(_) => Err(MagnusError::new(
                ruby.exception_runtime_error(),
                "a trapped run has no outcome",
            )),
        }
    }

    /// Unraised, so the Sandbox layer can attach the run's Execution before
    /// raising it.
    fn trap_error(&self) -> Result<Option<Exception>, MagnusError> {
        let Completion::Trap(trap) = &self.0.completion else {
            return Ok(None);
        };
        let ruby = Ruby::get().expect("Ruby thread");
        errors::trap_class(&ruby, trap)
            .new_instance((trap.to_string(),))
            .map(Some)
    }

    fn wall_time(&self) -> f64 {
        self.0.usage.wall_time
    }

    fn memory_peak(&self) -> usize {
        self.0.usage.memory_peak
    }

    fn stdout(&self) -> RString {
        let ruby = Ruby::get().expect("Ruby thread");
        ruby.str_from_slice(&self.0.stdout.bytes)
    }

    fn stdout_truncated(&self) -> bool {
        self.0.stdout.truncated
    }

    fn stderr(&self) -> RString {
        let ruby = Ruby::get().expect("Ruby thread");
        ruby.str_from_slice(&self.0.stderr.bytes)
    }

    fn stderr_truncated(&self) -> bool {
        self.0.stderr.truncated
    }
}
