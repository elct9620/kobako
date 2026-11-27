//! The lock a directory's calls run under, as Linux's `rw_semaphore` in
//! `kernel/locking`: shared while a name in it is looked up, exclusive while
//! one is made, removed, or moved. The other locks beneath the VFS sit
//! beside it.

pub(crate) mod seqlock;

use crate::sched::TaskId;
use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use crate::sched;
use crate::Errno;

/// How long the first waiter waits before the lock is handed to it, as
/// `RWSEM_WAIT_TIMEOUT` is a jiffy at 250 Hz.
const RWSEM_WAIT_TIMEOUT: Duration = Duration::from_millis(4);

/// The most readers one release lets in together, as `MAX_READERS_WAKEUP`.
const MAX_READERS_WAKEUP: usize = 256;

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

impl From<DownError> for Errno {
    fn from(error: DownError) -> Self {
        match error {
            DownError::Deadlock => Self::Deadlock,
            DownError::Interrupted => Self::Interrupted,
        }
    }
}

/// Held shared by any number of threads or exclusive by one, each waiter
/// served in turn, as Linux's rwsem. A thread arriving takes it ahead of the
/// waiters only where nobody holds it and none was handed it; a reader first
/// in line lets every waiting reader in with it; and a first waiter past
/// `RWSEM_WAIT_TIMEOUT` is handed it, so nobody arriving later keeps it out.
/// A thread asking again for a hold it could only get once it let go of its
/// own is refused as a Deadlock; a thread already holding it shared may hold
/// it shared again, past any waiter, which waits for that thread. Every wait
/// for it ends once its run is to stop, as each wait of Linux's is
/// `_killable`.
#[derive(Default)]
pub(crate) struct RwSemaphore {
    state: Mutex<State>,
    freed: Condvar,
}

/// What rwsem keeps in its `count`, `owner`, and `wait_list`, under one lock.
/// `readers`, `granted`, and `tickets` go beyond that shape, for a
/// Condvar to stand in for Linux's wakeups and for a task to be refused
/// what only its own shared hold keeps out; they may yet move to a structure
/// of their own.
#[derive(Default)]
struct State {
    /// The writer holding it, as a writer puts its task into rwsem's `owner`.
    owner: Option<TaskId>,
    /// One entry for each shared hold, a thread holding it shared twice
    /// standing twice. Linux keeps only a hint of the last reader, since
    /// tracking every one costs too much there; each is kept here so a task
    /// holding it shared is refused it exclusive, and takes it shared again
    /// past a waiting writer that waits on that task anyway.
    readers: Vec<TaskId>,
    /// Who waits, first in line first, as rwsem's `wait_list`.
    wait_list: VecDeque<Waiter>,
    /// The waiters a release let in, holding it before they woke, as
    /// `rwsem_mark_wake` grants a reader before it runs.
    granted: Vec<u64>,
    /// Whether the first waiter was handed it, so it alone may take it next,
    /// as `RWSEM_FLAG_HANDOFF`. It stays with the line while anyone waits.
    handoff: bool,
    tickets: u64,
}

struct Waiter {
    ticket: u64,
    task: TaskId,
    exclusive: bool,
    /// When it is handed the lock, if it stands first and still waits.
    timeout: Instant,
}

/// The semaphore while one call holds it, freed as the call returns. It keeps
/// the semaphore alive itself, so whatever a lock was reached through may go
/// first.
pub(crate) struct RwSemaphoreGuard {
    semaphore: Arc<RwSemaphore>,
    owner: TaskId,
    exclusive: bool,
}

impl State {
    fn free(&self) -> bool {
        self.owner.is_none() && self.readers.is_empty()
    }

    /// Whether the first waiter holds it at once, letting every waiting reader
    /// in with a reader.
    fn take_first(&mut self) -> bool {
        let Some(first) = self.wait_list.front() else {
            return false;
        };
        let (task, exclusive) = (first.task, first.exclusive);
        if self.owner.is_some() || (exclusive && !self.readers.is_empty()) {
            return false;
        }
        self.wait_list.pop_front();
        self.handoff = false;
        if exclusive {
            self.owner = Some(task);
        } else {
            self.readers.push(task);
            self.grant_readers(MAX_READERS_WAKEUP - 1);
        }
        true
    }

    /// Let up to `most` waiting readers in, wherever they stand in line, as
    /// `rwsem_mark_wake` wakes the readers once one stands first.
    fn grant_readers(&mut self, most: usize) {
        let mut left = most;
        self.wait_list.retain(|waiter| {
            if waiter.exclusive || left == 0 {
                return true;
            }
            left -= 1;
            self.readers.push(waiter.task);
            self.granted.push(waiter.ticket);
            false
        });
        self.handoff &= !self.wait_list.is_empty();
    }

    /// Whether a release let the waiter `ticket` in.
    fn take_granted(&mut self, ticket: u64) -> bool {
        let Some(at) = self.granted.iter().position(|held| *held == ticket) else {
            return false;
        };
        self.granted.swap_remove(at);
        true
    }

    /// Take the waiter `ticket` out of line, the handoff staying with whoever
    /// still waits, as `rwsem_del_waiter` clears it only once nobody does.
    fn leave(&mut self, ticket: u64) {
        self.wait_list.retain(|waiter| waiter.ticket != ticket);
        self.handoff &= !self.wait_list.is_empty();
    }
}

impl RwSemaphore {
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Hold it shared, waiting while a writer holds it or the line keeps the
    /// thread out.
    pub(crate) fn down_read(self: &Arc<Self>) -> Result<RwSemaphoreGuard, DownError> {
        let current = sched::current();
        let mut state = self.state();
        match state.owner {
            Some(writer) if writer == current => return Err(DownError::Deadlock),
            Some(_) => {}
            None if state.readers.contains(&current) || state.wait_list.is_empty() => {
                state.readers.push(current);
                return Ok(self.guard(current, false));
            }
            // A reader steals it from the line only where it would hold it alone,
            // as `rwsem_down_read_slowpath` steals, then lets in the readers
            // standing first.
            None if state.readers.is_empty() && !state.handoff => {
                state.readers.push(current);
                if state
                    .wait_list
                    .front()
                    .is_some_and(|first| !first.exclusive)
                {
                    state.grant_readers(MAX_READERS_WAKEUP - 1);
                    self.freed.notify_all();
                }
                return Ok(self.guard(current, false));
            }
            None => {}
        }
        self.down_slowpath(state, current, false)
    }

    /// Hold it exclusive, waiting while anyone holds it or the line keeps the
    /// thread out.
    pub(crate) fn down_write(self: &Arc<Self>) -> Result<RwSemaphoreGuard, DownError> {
        let current = sched::current();
        let mut state = self.state();
        if state.owner == Some(current) || state.readers.contains(&current) {
            return Err(DownError::Deadlock);
        }
        // A writer steals it nobody holds unless it was handed on, as
        // `rwsem_optimistic_spin` takes it.
        if state.free() && !state.handoff {
            state.owner = Some(current);
            return Ok(self.guard(current, true));
        }
        self.down_slowpath(state, current, true)
    }

    /// Hold it shared where nobody holds it exclusive or waits for it, without
    /// waiting, as `down_read_trylock` fails on `RWSEM_READ_FAILED_MASK`.
    pub(crate) fn down_read_trylock(self: &Arc<Self>) -> Option<RwSemaphoreGuard> {
        let current = sched::current();
        let mut state = self.state();
        if state.owner.is_some() || !state.wait_list.is_empty() {
            return None;
        }
        state.readers.push(current);
        Some(self.guard(current, false))
    }

    /// Hold it exclusive where nobody holds it and none was handed it, without
    /// waiting, as `mutex_trylock` fails only on an owner or a handoff: its
    /// one caller takes `rename_mutex`, which Linux keeps as a mutex.
    pub(crate) fn down_write_trylock(self: &Arc<Self>) -> Option<RwSemaphoreGuard> {
        let current = sched::current();
        let mut state = self.state();
        if !state.free() || state.handoff {
            return None;
        }
        state.owner = Some(current);
        Some(self.guard(current, true))
    }

    fn guard(self: &Arc<Self>, owner: TaskId, exclusive: bool) -> RwSemaphoreGuard {
        RwSemaphoreGuard {
            semaphore: self.clone(),
            owner,
            exclusive,
        }
    }

    /// Wait in line until the thread holds it, as `rwsem_down_read_slowpath`
    /// and `rwsem_down_write_slowpath` call `schedule`, until the run is to
    /// stop, as a killable wait looks at `fatal_signal_pending` each time it
    /// wakes. A hold a release granted counts even as the run stops, as Linux
    /// keeps a lock granted beside a fatal signal; a signal handled keeps the
    /// waiter's place.
    fn down_slowpath(
        self: &Arc<Self>,
        mut state: MutexGuard<'_, State>,
        owner: TaskId,
        exclusive: bool,
    ) -> Result<RwSemaphoreGuard, DownError> {
        state.tickets += 1;
        let ticket = state.tickets;
        state.wait_list.push_back(Waiter {
            ticket,
            task: owner,
            exclusive,
            timeout: Instant::now() + RWSEM_WAIT_TIMEOUT,
        });
        drop(state);
        let mut taken = Ok(());
        sched::schedule(&mut || {
            let mut state = self.state();
            loop {
                if state.take_granted(ticket) {
                    break;
                }
                let first = state
                    .wait_list
                    .front()
                    .filter(|first| first.ticket == ticket);
                let timeout = first.map(|first| first.timeout);
                if first.is_some() && state.take_first() {
                    if !exclusive {
                        self.freed.notify_all();
                    }
                    break;
                }
                if sched::fatal_signal_pending() {
                    state.leave(ticket);
                    self.freed.notify_all();
                    taken = Err(DownError::Interrupted);
                    break;
                }
                if sched::signal_pending() {
                    drop(state);
                    sched::handle_signal();
                    state = self.state();
                    continue;
                }
                let mut slice = sched::sleep_slice();
                if let Some(timeout) = timeout.filter(|_| !state.handoff) {
                    let now = Instant::now();
                    if now >= timeout {
                        state.handoff = true;
                        continue;
                    }
                    slice = slice.min(timeout - now);
                }
                state = self
                    .freed
                    .wait_timeout(state, slice)
                    .unwrap_or_else(PoisonError::into_inner)
                    .0;
            }
        });
        taken.map(|()| self.guard(owner, exclusive))
    }

    /// How many threads wait in line, for a test to see each one queued.
    #[cfg(test)]
    pub(crate) fn queued(&self) -> usize {
        self.state().wait_list.len()
    }

    /// Whether the first waiter was handed it, for a test to see the handoff.
    #[cfg(test)]
    pub(crate) fn handed_off(&self) -> bool {
        self.state().handoff
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
        let mut state = self.semaphore.state();
        if self.exclusive {
            state.owner = None;
        } else if let Some(at) = state
            .readers
            .iter()
            .position(|reader| *reader == self.owner)
        {
            state.readers.swap_remove(at);
        }
        let waiting = !state.wait_list.is_empty();
        drop(state);
        if waiting {
            self.semaphore.freed.notify_all();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sched::testing::{join, on_signal, wait_until, within_max_wait};
    use crate::sched::Task;
    use std::sync::mpsc;
    use std::thread::{self, JoinHandle};

    type Order = Arc<Mutex<Vec<&'static str>>>;

    /// A thread asking for the semaphore that the test may stop where it
    /// handles a signal, then let go on.
    struct Asker<T> {
        task: Task,
        stopped: mpsc::Receiver<()>,
        go: mpsc::Sender<()>,
        thread: JoinHandle<T>,
    }

    impl<T: Send + 'static> Asker<T> {
        /// Run `ask` on its own thread, signalled before it asks where
        /// `signalled`, so it stops as it first handles the signal.
        fn spawn(signalled: bool, ask: impl FnOnce() -> T + Send + 'static) -> Self {
            let (stopped_tx, stopped) = mpsc::channel();
            let (go, go_rx) = mpsc::channel::<()>();
            let (task_tx, task_rx) = mpsc::channel();
            let thread = thread::spawn(move || {
                on_signal(move || {
                    stopped_tx.send(()).expect("the test waits for the stop");
                    go_rx.recv().expect("the test lets the thread go on");
                });
                within_max_wait(|| {
                    task_tx
                        .send(Task::current())
                        .expect("the test takes the task");
                    if signalled {
                        Task::current().signal();
                    }
                    ask()
                })
            });
            let task = task_rx.recv().expect("the thread hands its task over");
            Self {
                task,
                stopped,
                go,
                thread,
            }
        }

        fn wait_stopped(&self, what: &str) {
            self.stopped
                .recv_timeout(sched::testing::MAX_WAIT)
                .unwrap_or_else(|_| {
                    panic!(
                        "{what} did not happen within the max wait; check that the signal \
                         reached the waiter and that its wait handles one"
                    )
                });
        }

        fn go_on(&self) {
            self.go.send(()).expect("the thread waits to go on");
        }

        fn join(self, what: &str) -> T {
            join(what, self.thread)
        }
    }

    /// Ask for it exclusive, or shared, noting `name` in `order` while holding it.
    fn note(
        rwsem: &Arc<RwSemaphore>,
        exclusive: bool,
        order: &Order,
        name: &'static str,
    ) -> impl FnOnce() -> Result<(), DownError> + Send + 'static {
        let (rwsem, order) = (rwsem.clone(), order.clone());
        move || {
            let _held = if exclusive {
                rwsem.down_write()?
            } else {
                rwsem.down_read()?
            };
            order.lock().expect("never poisoned").push(name);
            Ok(())
        }
    }

    fn wait_until_queued(rwsem: &Arc<RwSemaphore>, count: usize) {
        wait_until(&format!("{count} threads waiting in line"), || {
            rwsem.queued() == count
        });
    }

    // Holding it shared again past the waiting writer stands in for what a walk
    // does when a Ruby filesystem reaches its own mount, which must never hang.
    // @behavior RT-253
    #[test]
    fn a_writer_goes_ahead_of_readers_arriving_after_it() {
        let rwsem: Arc<RwSemaphore> = Arc::default();
        let order = Order::default();
        let held = rwsem.down_read().expect("the first reader holds it");
        let writer = thread::spawn(bound_to_max_wait(note(&rwsem, true, &order, "writer")));
        wait_until_queued(&rwsem, 1);
        let reader = thread::spawn(bound_to_max_wait(note(&rwsem, false, &order, "reader")));
        wait_until_queued(&rwsem, 2);
        let again = rwsem.down_read().map(drop);
        drop(held);
        let answers = (join("the writer", writer), join("the reader", reader));
        assert_eq!(
            (
                again,
                answers,
                order.lock().expect("never poisoned").clone()
            ),
            (Ok(()), (Ok(()), Ok(())), vec!["writer", "reader"]),
            "a reader arriving at a held directory behind a waiting writer must wait \
             for it, as rwsem_down_read_slowpath queues, while one already holding it \
             takes it again"
        );
    }

    fn bound_to_max_wait<T>(ask: impl FnOnce() -> T + Send) -> impl FnOnce() -> T + Send {
        move || within_max_wait(ask)
    }

    // @behavior RT-420
    #[test]
    fn a_reader_takes_a_free_directory_past_a_writer_waiting_for_it() {
        let rwsem: Arc<RwSemaphore> = Arc::default();
        let held = rwsem.down_write().expect("the owner holds it");
        let writer = Asker::spawn(true, {
            let rwsem = rwsem.clone();
            move || rwsem.down_write().map(drop)
        });
        writer.wait_stopped("the writer stopping as it handles a signal");
        drop(held);
        let reader = within_max_wait(|| rwsem.down_read().map(drop));
        writer.go_on();
        assert_eq!(
            (reader, writer.join("the writer")),
            (Ok(()), Ok(())),
            "a reader asking for a directory nobody holds must take it past a waiting \
             writer never handed it, as rwsem_down_read_slowpath steals it"
        );
    }

    // @behavior RT-421
    #[test]
    fn a_release_with_a_reader_first_lets_every_waiting_reader_in() {
        let rwsem: Arc<RwSemaphore> = Arc::default();
        let order = Order::default();
        let held = rwsem.down_write().expect("the owner holds it");
        let first = thread::spawn(bound_to_max_wait(note(&rwsem, false, &order, "reader")));
        wait_until_queued(&rwsem, 1);
        let writer = thread::spawn(bound_to_max_wait(note(&rwsem, true, &order, "writer")));
        wait_until_queued(&rwsem, 2);
        let last = thread::spawn(bound_to_max_wait(note(&rwsem, false, &order, "reader")));
        wait_until_queued(&rwsem, 3);
        drop(held);
        let answers = (
            join("the first reader", first),
            join("the writer", writer),
            join("the last reader", last),
        );
        assert_eq!(
            (answers, order.lock().expect("never poisoned").clone()),
            ((Ok(()), Ok(()), Ok(())), vec!["reader", "reader", "writer"]),
            "a release with a reader first in line must let every waiting reader in \
             before the writer between them, as rwsem_mark_wake wakes the readers"
        );
    }

    // @behavior RT-422
    #[test]
    fn a_waiter_past_its_time_is_handed_the_directory() {
        let rwsem: Arc<RwSemaphore> = Arc::default();
        let order = Order::default();
        let held = rwsem.down_write().expect("the owner holds it");
        let first = Asker::spawn(false, note(&rwsem, true, &order, "first"));
        wait_until("the first waiter being handed it", || rwsem.handed_off());
        first.task.signal();
        first.wait_stopped("the first waiter stopping as it handles a signal");
        drop(held);
        let tried = (
            rwsem.down_write_trylock().is_some(),
            rwsem.down_read_trylock().is_some(),
        );
        let later = thread::spawn(bound_to_max_wait(note(&rwsem, true, &order, "later")));
        wait_until_queued(&rwsem, 2);
        first.go_on();
        let answers = (
            first.join("the first waiter"),
            join("the later thread", later),
        );
        assert_eq!(
            (
                tried,
                answers,
                order.lock().expect("never poisoned").clone()
            ),
            ((false, false), (Ok(()), Ok(())), vec!["first", "later"]),
            "a directory handed to a waiter past RWSEM_WAIT_TIMEOUT must go to it before \
             a thread arriving later, waiting or by trylock"
        );
    }

    // @behavior RT-423
    #[test]
    fn a_waiter_leaving_with_the_handoff_passes_it_on() {
        let rwsem: Arc<RwSemaphore> = Arc::default();
        let order = Order::default();
        let held = rwsem.down_write().expect("the owner holds it");
        let first = Asker::spawn(false, note(&rwsem, true, &order, "first"));
        wait_until("the first waiter being handed it", || rwsem.handed_off());
        let behind = Asker::spawn(true, note(&rwsem, true, &order, "behind"));
        behind.wait_stopped("the waiter behind stopping as it handles a signal");
        first.task.interrupt();
        let left = first.join("the first waiter");
        wait_until_queued(&rwsem, 1);
        drop(held);
        let later = thread::spawn(bound_to_max_wait(note(&rwsem, true, &order, "later")));
        wait_until_queued(&rwsem, 2);
        behind.go_on();
        let answers = (
            behind.join("the waiter behind"),
            join("the later thread", later),
        );
        assert_eq!(
            (left, answers, order.lock().expect("never poisoned").clone()),
            (
                Err(DownError::Interrupted),
                (Ok(()), Ok(())),
                vec!["behind", "later"]
            ),
            "a waiter interrupted with the handoff must leave it to the waiter behind, \
             as rwsem_del_waiter keeps RWSEM_FLAG_HANDOFF while anyone waits"
        );
    }

    // @behavior RT-424
    #[test]
    fn a_waiter_granted_as_it_is_interrupted_lets_the_directory_go() {
        let rwsem: Arc<RwSemaphore> = Arc::default();
        let order = Order::default();
        let held = rwsem.down_write().expect("the owner holds it");
        let first = thread::spawn(bound_to_max_wait(note(&rwsem, false, &order, "first")));
        wait_until_queued(&rwsem, 1);
        let granted = Asker::spawn(true, note(&rwsem, false, &order, "granted"));
        granted.wait_stopped("the reader behind stopping as it handles a signal");
        drop(held);
        let first = join("the first reader", first);
        granted.task.interrupt();
        granted.go_on();
        let granted = granted.join("the granted reader");
        let writer = within_max_wait(|| rwsem.down_write().map(drop));
        assert_eq!(
            (first, granted, writer),
            (Ok(()), Ok(()), Ok(())),
            "a reader granted a directory as it is interrupted must hold it, then let \
             it go, so a writer takes it after"
        );
    }
}
