//! The lock an Overlay holds across each call, so a change that touches
//! several Layers is never seen, nor interleaved with, half made, as the
//! directory's lock overlayfs's changes run under.

use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::{self, ThreadId};

use super::super::FilesystemError;

/// How a thread waits for a lock another thread holds. A frontend whose
/// threads share a lock of their own, as Ruby's threads share the GVL,
/// gives that lock up for the wait, or a holder that needs it to finish
/// never could.
pub trait Blocking: Send + Sync {
    /// Run `wait`, which returns once the lock is taken.
    fn wait(&self, wait: &mut dyn FnMut());
}

/// Waiting with nothing else to give up.
pub(super) struct Inline;

impl Blocking for Inline {
    fn wait(&self, wait: &mut dyn FnMut()) {
        wait();
    }
}

/// Held by one thread at a time. The thread holding it asking again is
/// refused as a deadlock, as `fcntl` refuses a lock that could never be
/// granted.
pub(super) struct Exclusive {
    holder: Mutex<Option<ThreadId>>,
    freed: Condvar,
    blocking: Arc<dyn Blocking>,
}

impl Exclusive {
    pub(super) fn new(blocking: Arc<dyn Blocking>) -> Self {
        Self {
            holder: Mutex::new(None),
            freed: Condvar::new(),
            blocking,
        }
    }

    pub(super) fn blocking(&self) -> Arc<dyn Blocking> {
        self.blocking.clone()
    }

    fn holder(&self) -> MutexGuard<'_, Option<ThreadId>> {
        self.holder
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub(super) fn hold(&self) -> Result<Held<'_>, FilesystemError> {
        let me = thread::current().id();
        let mut holder = self.holder();
        match *holder {
            Some(held) if held == me => return Err(FilesystemError::Deadlock),
            Some(_) => {
                drop(holder);
                self.blocking.wait(&mut || {
                    let mut holder = self.holder();
                    while holder.is_some() {
                        holder = self
                            .freed
                            .wait(holder)
                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                    }
                    *holder = Some(me);
                });
            }
            None => *holder = Some(me),
        }
        Ok(Held(self))
    }
}

/// The lock while one call holds it, freed as the call returns.
pub(super) struct Held<'a>(&'a Exclusive);

impl Drop for Held<'_> {
    fn drop(&mut self) {
        *self.0.holder() = None;
        self.0.freed.notify_one();
    }
}
