//! A lock its readers never wait on, as Linux's `seqlock_t`: a writer
//! counts each write, and a reader that a write overlapped reads again.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};

pub(crate) struct SeqLock {
    /// Holds one writer at a time, as `write_seqlock` takes the lock's
    /// spinlock.
    writer: Mutex<()>,
    /// How many times a write began or ended, odd while one is under way.
    sequence: AtomicUsize,
}

impl SeqLock {
    pub(crate) const fn new() -> Self {
        Self {
            writer: Mutex::new(()),
            sequence: AtomicUsize::new(0),
        }
    }

    /// Run `write` with every reader overlapping it told to read again, as
    /// `write_seqlock` and `write_sequnlock` bracket a write.
    pub(crate) fn write<T>(&self, write: impl FnOnce() -> T) -> T {
        let _held = self.writer.lock().unwrap_or_else(PoisonError::into_inner);
        self.sequence.fetch_add(1, Ordering::SeqCst);
        let written = write();
        self.sequence.fetch_add(1, Ordering::SeqCst);
        written
    }

    /// What `read` answers with no write overlapping it, as
    /// `read_seqbegin_or_lock` reads: first without waiting, then, once a
    /// write overlapped, holding writers off, so writes one after another
    /// never keep a reader reading again.
    pub(crate) fn read<T>(&self, mut read: impl FnMut() -> T) -> T {
        let begun = self.sequence.load(Ordering::SeqCst);
        if begun.is_multiple_of(2) {
            let answer = read();
            if self.sequence.load(Ordering::SeqCst) == begun {
                return answer;
            }
        }
        let _held = self.writer.lock().unwrap_or_else(PoisonError::into_inner);
        read()
    }
}
