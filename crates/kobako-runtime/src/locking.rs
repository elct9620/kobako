//! The lock a directory's calls run under, as Linux's `rw_semaphore` in
//! `kernel/locking`: shared while a name in it is looked up, exclusive while
//! one is made, removed, or moved.

use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::ThreadId;

use crate::sched;

/// A wait no release could ever end, since the thread asking holds what it
/// asks for. A thread must never hang on itself, so it is answered, as
/// Linux's `rt_mutex` answers `-EDEADLK`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Deadlock;

/// Held shared by any number of threads or exclusive by one. A thread
/// asking again for a hold it could only get once it let go of its own is
/// refused as a Deadlock; a thread already holding it shared may hold it
/// shared again.
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

/// The semaphore while one call holds it, freed as the call returns. It keeps
/// the semaphore alive itself, so whatever a lock was reached through may go
/// first.
pub(crate) struct RwSemaphoreGuard {
    semaphore: Arc<RwSemaphore>,
    holder: ThreadId,
    exclusive: bool,
}

impl RwSemaphore {
    fn holders(&self) -> MutexGuard<'_, Holders> {
        self.holders.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Hold it shared, waiting while a writer holds it.
    pub(crate) fn down_read(self: &Arc<Self>) -> Result<RwSemaphoreGuard, Deadlock> {
        let current = sched::current();
        self.down(current, false, |holders| match holders.writer {
            Some(writer) if writer == current => Err(Deadlock),
            Some(_) => Ok(false),
            None => {
                holders.readers.push(current);
                Ok(true)
            }
        })
    }

    /// Hold it exclusive, waiting while anyone holds it.
    pub(crate) fn down_write(self: &Arc<Self>) -> Result<RwSemaphoreGuard, Deadlock> {
        let current = sched::current();
        self.down(current, true, |holders| {
            if holders.writer == Some(current) || holders.readers.contains(&current) {
                return Err(Deadlock);
            }
            if holders.writer.is_some() || !holders.readers.is_empty() {
                return Ok(false);
            }
            holders.writer = Some(current);
            Ok(true)
        })
    }

    /// Take the hold `take` grants, or wait for it.
    fn down(
        self: &Arc<Self>,
        holder: ThreadId,
        exclusive: bool,
        mut take: impl FnMut(&mut Holders) -> Result<bool, Deadlock>,
    ) -> Result<RwSemaphoreGuard, Deadlock> {
        let mut holders = self.holders();
        if !take(&mut holders)? {
            drop(holders);
            self.down_slowpath(&mut take)?;
        }
        Ok(RwSemaphoreGuard {
            semaphore: self.clone(),
            holder,
            exclusive,
        })
    }

    /// Wait for a release and ask again, as `rwsem_down_read_slowpath` and
    /// `rwsem_down_write_slowpath` call `schedule`.
    fn down_slowpath(
        &self,
        take: &mut impl FnMut(&mut Holders) -> Result<bool, Deadlock>,
    ) -> Result<(), Deadlock> {
        let mut taken = Ok(());
        sched::schedule(&mut || {
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
        taken
    }
}

impl Drop for RwSemaphoreGuard {
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
