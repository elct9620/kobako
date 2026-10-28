//! The lock an Overlay holds across each call, so a change that touches
//! several Layers is never seen, nor interleaved with, half made, as the
//! directory's lock overlayfs's changes run under.

use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::{self, ThreadId};

use super::super::FilesystemError;
use crate::vfs::Blocking;

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

    pub(super) fn hold(&self) -> Result<ExclusiveGuard<'_>, FilesystemError> {
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
        Ok(ExclusiveGuard(self))
    }
}

/// The lock while one call holds it, freed as the call returns.
pub(super) struct ExclusiveGuard<'a>(&'a Exclusive);

impl Drop for ExclusiveGuard<'_> {
    fn drop(&mut self) {
        *self.0.holder() = None;
        self.0.freed.notify_one();
    }
}
