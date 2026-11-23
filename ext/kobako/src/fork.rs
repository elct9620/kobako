//! A fork through Ruby, checked before it happens, as `pthread_atfork`'s
//! prepare handler runs in the forking thread. A forked child inherits the
//! memory but only the forking thread, so a fork waits for every run in
//! progress to end, and raises where it cannot: on a Thread inside a run,
//! which waiting would wait on, and on macOS once the system verified a
//! certificate, after which a child touching its certificate services
//! crashes.

use std::sync::atomic::{AtomicBool, Ordering};

use kobako_runtime::filesystem::Bind;
use kobako_runtime::wait::do_poll;
use magnus::{function, prelude::*, Error as MagnusError, RModule, Ruby, Value};

use super::escape::{self, ForkGate};

pub(crate) fn register(kobako: RModule) -> Result<(), MagnusError> {
    let fork = kobako.define_module("Fork")?;
    fork.define_singleton_method("prepare", function!(prepare, 0))?;
    fork.define_singleton_method("release", function!(escape::let_runs_in, 0))?;
    Ok(())
}

/// Wait until no run is in progress, then hold new ones back until
/// `release`, raising `RuntimeError` for a fork that cannot go on. Once held
/// back, nothing walks a Bind, so the directories Binds keep are let go and
/// the child inherits none of them.
fn prepare() -> Result<(), MagnusError> {
    let ruby = Ruby::get().expect("a fork's prepare runs on a Ruby thread");
    refuse_past_trust(&ruby)?;
    // SAFETY: `rb_thread_current` reads the calling thread's own execution
    // context, which a thread holding the GVL may.
    let thread = unsafe { rb_sys::rb_thread_current() };
    let mut warned = false;
    loop {
        match escape::hold_runs_back(thread, None) {
            ForkGate::Held => return held_back(),
            ForkGate::OnThread => {
                return Err(MagnusError::new(
                    ruby.exception_runtime_error(),
                    "cannot fork while this Thread is inside a Sandbox run, which the fork \
                     would wait on; fork before the run starts or after it ends",
                ))
            }
            ForkGate::Busy if !warned => {
                warned = true;
                let _: Value = ruby.module_kernel().funcall(
                    "warn",
                    ("kobako: fork waits for the Sandbox runs on other Threads to end",),
                )?;
            }
            ForkGate::Busy => {}
        }
        let waited = do_poll(
            || None,
            |table| match escape::hold_runs_back(thread, table) {
                ForkGate::Busy => None,
                gate => Some(gate),
            },
        );
        match waited {
            Ok(Some(ForkGate::Held)) => return held_back(),
            Ok(_) => {}
            Err(_) => ruby.thread_check_ints()?,
        }
    }
}

/// Runs are held back for the fork, so let go of what the child would inherit
/// for nothing.
fn held_back() -> Result<(), MagnusError> {
    Bind::drop_kept_directories();
    Ok(())
}

/// Tell Pitchfork, once, that this process forks no more once the system
/// verified a certificate, so it forks from another worker rather than meet
/// the refusal. The guest's thread that verified holds no
/// GVL, so the run tells it as it ends.
pub(crate) fn tell_pitchfork_past_trust(ruby: &Ruby) {
    static TOLD: AtomicBool = AtomicBool::new(false);
    if kobako_runtime::net::first_trusted_host().is_none() || TOLD.load(Ordering::Relaxed) {
        return;
    }
    let info = ruby
        .class_object()
        .const_get::<_, RModule>("Pitchfork")
        .and_then(|pitchfork| pitchfork.const_get::<_, Value>("Info"));
    if let Ok(info) = info {
        TOLD.store(true, Ordering::Relaxed);
        let _: Result<Value, _> = info.funcall("no_longer_fork_safe!", ());
    }
}

/// Raise for a fork on macOS once the system verified a certificate.
fn refuse_past_trust(ruby: &Ruby) -> Result<(), MagnusError> {
    match kobako_runtime::net::first_trusted_host() {
        Some(host) => Err(MagnusError::new(
            ruby.exception_runtime_error(),
            format!(
                "cannot fork after a guest's HTTPS to {host} had macOS verify a certificate: \
                 a child touching the system's certificate services crashes; fork before any \
                 guest makes an HTTPS request"
            ),
        )),
        None => Ok(()),
    }
}
