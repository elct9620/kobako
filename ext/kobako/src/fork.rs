//! A fork through Ruby, checked before it happens, as `pthread_atfork`'s
//! prepare handler runs in the forking thread. A forked child inherits the
//! memory but only the forking thread, so a fork raises while any run is in
//! progress, whose locks the child would inherit held, and on macOS once the
//! system verified a certificate, after which a child touching its
//! certificate services crashes.

use std::sync::atomic::{AtomicBool, Ordering};

use kobako_runtime::filesystem::Bind;
use magnus::{function, prelude::*, Error as MagnusError, RModule, Ruby, Value};

use super::escape;

pub(crate) fn register(kobako: RModule) -> Result<(), MagnusError> {
    let fork = kobako.define_module("Fork")?;
    fork.define_singleton_method("prepare", function!(prepare, 0))?;
    fork.define_singleton_method("release", function!(escape::let_runs_in, 0))?;
    Ok(())
}

/// Hold new runs back until `release`, raising `RuntimeError` for a fork that
/// cannot go on: the permanent refusal first, so no one retries it. Once held
/// back, nothing walks a Bind, so the directories Binds keep are let go and
/// the child inherits none of them.
fn prepare() -> Result<(), MagnusError> {
    let ruby = Ruby::get().expect("a fork's prepare runs on a Ruby thread");
    refuse_past_trust(&ruby)?;
    if !escape::hold_runs_back() {
        return Err(MagnusError::new(
            ruby.exception_runtime_error(),
            "cannot fork while a Sandbox run is in progress, as the child would inherit \
             the locks it holds; fork again once no run is in progress",
        ));
    }
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
