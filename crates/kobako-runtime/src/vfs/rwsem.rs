//! The lock a directory's calls run under, as Linux's `rw_semaphore`
//! behind each directory's `i_rwsem`: shared while a name in it is looked
//! up, exclusive while one is made, removed, or moved.

use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::{self, ThreadId};

use crate::filesystem::FilesystemError;

/// How a thread waits for a lock another thread holds. A frontend whose
/// threads share a lock of their own, as Ruby's threads share the GVL,
/// gives that lock up for the wait, or a holder that needs it to finish
/// never could.
pub trait Blocking: Send + Sync {
    /// Run `wait`, which returns once the lock is taken.
    fn wait(&self, wait: &mut dyn FnMut());
}

/// Waiting with nothing else to give up.
pub(crate) struct Inline;

impl Blocking for Inline {
    fn wait(&self, wait: &mut dyn FnMut()) {
        wait();
    }
}

/// Held shared by any number of threads or exclusive by one. A thread
/// asking again for a hold it could only get once it let go of its own is
/// refused as a deadlock, as `fcntl` refuses a lock that could never be
/// granted; a thread already holding it shared may hold it shared again.
#[derive(Default)]
pub(crate) struct RwSemaphore {
    holders: Mutex<Holders>,
    freed: Condvar,
}

#[derive(Default)]
struct Holders {
    writer: Option<ThreadId>,
    /// One entry for each shared hold, a thread holding it shared twice
    /// standing twice.
    readers: Vec<ThreadId>,
    /// How many threads wait for a release, so one nobody waits for wakes
    /// no one.
    waiting: usize,
}

/// The semaphore while one call holds it, freed as the call returns.
pub(crate) struct Held<'a> {
    semaphore: &'a RwSemaphore,
    holder: ThreadId,
    exclusive: bool,
}

thread_local! {
    static ME: ThreadId = thread::current().id();
}

fn me() -> ThreadId {
    ME.with(|me| *me)
}

impl RwSemaphore {
    fn holders(&self) -> MutexGuard<'_, Holders> {
        self.holders.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Hold it shared, waiting through `blocking` while a writer holds it.
    pub(crate) fn down_read(&self, blocking: &dyn Blocking) -> Result<Held<'_>, FilesystemError> {
        let me = me();
        self.down(blocking, me, false, |holders| match holders.writer {
            Some(writer) if writer == me => Err(FilesystemError::Deadlock),
            Some(_) => Ok(false),
            None => {
                holders.readers.push(me);
                Ok(true)
            }
        })
    }

    /// Hold it exclusive, waiting through `blocking` while anyone holds it.
    pub(crate) fn down_write(&self, blocking: &dyn Blocking) -> Result<Held<'_>, FilesystemError> {
        let me = me();
        self.down(blocking, me, true, |holders| {
            if holders.writer == Some(me) || holders.readers.contains(&me) {
                return Err(FilesystemError::Deadlock);
            }
            if holders.writer.is_some() || !holders.readers.is_empty() {
                return Ok(false);
            }
            holders.writer = Some(me);
            Ok(true)
        })
    }

    /// Take the hold `take` grants, or wait for a release and ask again.
    fn down(
        &self,
        blocking: &dyn Blocking,
        holder: ThreadId,
        exclusive: bool,
        mut take: impl FnMut(&mut Holders) -> Result<bool, FilesystemError>,
    ) -> Result<Held<'_>, FilesystemError> {
        let mut holders = self.holders();
        if !take(&mut holders)? {
            drop(holders);
            let mut taken = Ok(());
            blocking.wait(&mut || {
                let mut holders = self.holders();
                loop {
                    match take(&mut holders) {
                        Ok(true) => return,
                        Ok(false) => {
                            holders.waiting += 1;
                            holders = self
                                .freed
                                .wait(holders)
                                .unwrap_or_else(PoisonError::into_inner);
                            holders.waiting -= 1;
                        }
                        Err(err) => {
                            taken = Err(err);
                            return;
                        }
                    }
                }
            });
            taken?;
        }
        Ok(Held {
            semaphore: self,
            holder,
            exclusive,
        })
    }
}

impl Drop for Held<'_> {
    fn drop(&mut self) {
        let mut holders = self.semaphore.holders();
        if self.exclusive {
            holders.writer = None;
        } else if let Some(at) = holders
            .readers
            .iter()
            .position(|reader| *reader == self.holder)
        {
            holders.readers.swap_remove(at);
        }
        let waiting = holders.waiting > 0;
        drop(holders);
        if waiting {
            self.semaphore.freed.notify_all();
        }
    }
}
