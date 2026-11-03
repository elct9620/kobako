//! The lock a directory's calls run under, as Linux's `rw_semaphore` in
//! `kernel/locking`: shared while a name in it is looked up, exclusive while
//! one is made, removed, or moved.

use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::ThreadId;

use crate::sched;

/// Why a hold was not taken.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DownError {
    /// A wait no release could ever end, since the thread asking holds what it
    /// asks for. A thread must never hang on itself, so it is answered, as
    /// Linux's `rt_mutex` answers `-EDEADLK`.
    Deadlock,
    /// The wait's run was to stop, past its deadline or interrupted, as
    /// `down_read_killable` answers `-EINTR` on a fatal signal.
    Interrupted,
}

/// Held shared by any number of threads or exclusive by one. A thread
/// asking again for a hold it could only get once it let go of its own is
/// refused as a Deadlock; a thread already holding it shared may hold it
/// shared again, even past a waiting writer, which waits for that thread.
/// Every wait for it ends once its run is to stop, as each wait of Linux's
/// is `_killable`.
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
    /// How many threads wait to hold it exclusive, which a thread arriving to hold
    /// it shared goes behind, as `rwsem_down_read_slowpath` queues behind a waiting
    /// writer, so readers one after another never keep a writer out.
    writers_waiting: usize,
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
    pub(crate) fn down_read(self: &Arc<Self>) -> Result<RwSemaphoreGuard, DownError> {
        let current = sched::current();
        self.down(current, false, |holders| match holders.writer {
            Some(writer) if writer == current => Err(DownError::Deadlock),
            Some(_) => Ok(false),
            None if holders.writers_waiting > 0 && !holders.readers.contains(&current) => Ok(false),
            None => {
                holders.readers.push(current);
                Ok(true)
            }
        })
    }

    /// Hold it exclusive, waiting while anyone holds it.
    pub(crate) fn down_write(self: &Arc<Self>) -> Result<RwSemaphoreGuard, DownError> {
        let current = sched::current();
        self.down(current, true, |holders| {
            if holders.writer == Some(current) || holders.readers.contains(&current) {
                return Err(DownError::Deadlock);
            }
            if holders.writer.is_some() || !holders.readers.is_empty() {
                return Ok(false);
            }
            holders.writer = Some(current);
            Ok(true)
        })
    }

    /// Whether a thread waits to hold it exclusive, for a test to see one queued.
    #[cfg(test)]
    pub(crate) fn writer_waits(&self) -> bool {
        self.holders().writers_waiting > 0
    }

    /// Take the hold `take` grants, or wait for it.
    fn down(
        self: &Arc<Self>,
        holder: ThreadId,
        exclusive: bool,
        mut take: impl FnMut(&mut Holders) -> Result<bool, DownError>,
    ) -> Result<RwSemaphoreGuard, DownError> {
        let mut holders = self.holders();
        if !take(&mut holders)? {
            drop(holders);
            self.down_slowpath(exclusive, &mut take)?;
        }
        Ok(RwSemaphoreGuard {
            semaphore: self.clone(),
            holder,
            exclusive,
        })
    }

    /// Wait for a release and ask again, as `rwsem_down_read_slowpath` and
    /// `rwsem_down_write_slowpath` call `schedule`, until the run is to stop, as
    /// a killable wait looks at `fatal_signal_pending` each time it wakes.
    fn down_slowpath(
        &self,
        exclusive: bool,
        take: &mut impl FnMut(&mut Holders) -> Result<bool, DownError>,
    ) -> Result<(), DownError> {
        let mut taken = Ok(());
        sched::schedule(&mut || {
            let mut holders = self.holders();
            holders.writers_waiting += usize::from(exclusive);
            loop {
                match take(&mut holders) {
                    Ok(true) => break,
                    Ok(false) if sched::fatal_signal_pending() => {
                        taken = Err(DownError::Interrupted);
                        break;
                    }
                    Ok(false) => {
                        holders.waiting += 1;
                        holders = self
                            .freed
                            .wait_timeout(holders, sched::sleep_slice())
                            .unwrap_or_else(PoisonError::into_inner)
                            .0;
                        holders.waiting -= 1;
                    }
                    Err(err) => {
                        taken = Err(err);
                        break;
                    }
                }
            }
            holders.writers_waiting -= usize::from(exclusive);
            // A reader may have stood behind this writer, which now takes it or
            // gave up; one that took it wakes them again once it lets go.
            if holders.waiting > 0 && taken.is_err() {
                self.freed.notify_all();
            }
        });
        taken
    }
}

impl RwSemaphoreGuard {
    /// Whether this holds `semaphore`.
    pub(crate) fn holds(&self, semaphore: &Arc<RwSemaphore>) -> bool {
        Arc::ptr_eq(&self.semaphore, semaphore)
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
