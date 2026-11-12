//! Waiting for what another thread brings, as Linux's wait queues and
//! `do_poll` are: a producer wakes the queue its event stands on, and a
//! poller that registered on that queue looks again. A wait ends once its run
//! is to stop, as `do_poll`'s interruptible sleep ends on a pending signal; the
//! only signal a run receives is the one that stops it. It waits through the
//! registered Blocking, as a lock's slow path does.

use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::task::Waker;
use std::time::{Duration, Instant};

use crate::sched;
use crate::Errno;

/// Where the waiters for one event stand, as Linux's `wait_queue_head_t`.
#[derive(Default)]
pub struct WaitQueueHead {
    waiters: Mutex<Vec<Arc<Waiter>>>,
    /// The tasks waiting here, each woken once and then gone, as an entry
    /// `init_waitqueue_func_entry` makes with `autoremove_wake_function`.
    tasks: Mutex<Vec<Waker>>,
}

impl WaitQueueHead {
    /// Wake every waiter standing here, as `wake_up` does once the event
    /// arrived.
    pub fn wake_up(&self) {
        for waiter in self.waiters().iter() {
            waiter.wake();
        }
        let tasks = std::mem::take(&mut *self.tasks.lock().unwrap_or_else(PoisonError::into_inner));
        for task in tasks {
            task.wake();
        }
    }

    /// Have the task `waker` stands for woken by the next wake here, once, as
    /// `add_wait_queue` adds an entry whose wake function is the task's; a
    /// task already waiting is not added twice.
    pub(crate) fn add(&self, waker: &Waker) {
        let mut tasks = self.tasks.lock().unwrap_or_else(PoisonError::into_inner);
        if !tasks.iter().any(|task| task.will_wake(waker)) {
            tasks.push(waker.clone());
        }
    }

    fn waiters(&self) -> MutexGuard<'_, Vec<Arc<Waiter>>> {
        self.waiters.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// One sleeping poller, as a `poll_wqueues` entry: a wake that arrives
/// before it sleeps is kept, so none is lost.
#[derive(Default)]
pub(crate) struct Waiter {
    woken: Mutex<bool>,
    wake: Condvar,
}

impl Waiter {
    pub(crate) fn wake(&self) {
        *self.woken() = true;
        self.wake.notify_all();
    }

    /// Sleep until woken or `until` passes.
    fn sleep_until(&self, until: Option<Instant>) {
        let mut woken = self.woken();
        while !*woken {
            woken = match until {
                Some(at) => {
                    let left = at.saturating_duration_since(Instant::now());
                    if left.is_zero() {
                        break;
                    }
                    self.wake
                        .wait_timeout(woken, left)
                        .unwrap_or_else(PoisonError::into_inner)
                        .0
                }
                None => self
                    .wake
                    .wait(woken)
                    .unwrap_or_else(PoisonError::into_inner),
            };
        }
        *woken = false;
    }

    fn woken(&self) -> MutexGuard<'_, bool> {
        self.woken.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The queues one poll registered on, as Linux's `poll_table`; dropping it
/// leaves each, as `poll_freewait` does.
pub struct PollTable {
    waiter: Arc<Waiter>,
    queues: Vec<Arc<WaitQueueHead>>,
}

impl Drop for PollTable {
    fn drop(&mut self) {
        for queue in &self.queues {
            queue
                .waiters()
                .retain(|waiter| !Arc::ptr_eq(waiter, &self.waiter));
        }
    }
}

/// Have the poll `table` stands for woken by `queue`, as a file's `->poll`
/// calls `poll_wait`; a poll that will not sleep passes no table.
pub fn poll_wait(queue: &Arc<WaitQueueHead>, table: Option<&mut PollTable>) {
    if let Some(table) = table {
        queue.waiters().push(table.waiter.clone());
        table.queues.push(queue.clone());
    }
}

/// Ask `poll` until it finds something ready, `timeout` passes, or the run is
/// to stop, as Linux's `do_poll` asks each file again whenever a queue it
/// registered on wakes. The first ask registers through the table it is
/// handed; nothing ready by `timeout` answers `None`, and a run that is to stop
/// answers EINTR.
pub fn do_poll<T>(
    timeout: Option<Duration>,
    mut poll: impl FnMut(Option<&mut PollTable>) -> Option<T>,
) -> Result<Option<T>, Errno> {
    let end = timeout.map(|timeout| Instant::now() + timeout);
    let mut table = PollTable {
        waiter: Arc::default(),
        queues: Vec::new(),
    };
    if let Some(ready) = poll(Some(&mut table)) {
        return Ok(Some(ready));
    }
    let mut answer = Ok(None);
    sched::sleep_on(&table.waiter, || {
        sched::schedule(&mut || loop {
            if sched::fatal_signal_pending() {
                answer = Err(Errno::Interrupted);
                return;
            }
            if end.is_some_and(|end| Instant::now() >= end) {
                return;
            }
            let until = match (end, sched::deadline()) {
                (Some(end), Some(deadline)) => Some(end.min(deadline)),
                (end, deadline) => end.or(deadline),
            };
            table.waiter.sleep_until(until);
            if let Some(ready) = poll(None) {
                answer = Ok(Some(ready));
                return;
            }
        })
    });
    answer
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread;

    use super::*;
    use crate::sched::testing::{count_waits, join, within_max_wait};
    use crate::sched::{run_task, Task};

    // @behavior RT-335
    #[test]
    fn a_poll_looks_again_once_the_queue_it_registered_on_wakes() {
        let queue = Arc::new(WaitQueueHead::default());
        let ready = Arc::new(AtomicBool::new(false));
        let producer = {
            let (queue, ready) = (queue.clone(), ready.clone());
            thread::spawn(move || {
                while queue.waiters().is_empty() {
                    thread::yield_now();
                }
                // The poller has asked once and gone to sleep by now, so only
                // the wake makes it look again before its deadline.
                thread::sleep(Duration::from_millis(50));
                ready.store(true, Ordering::SeqCst);
                queue.wake_up();
            })
        };
        let started = Instant::now();
        let answer = within_max_wait(|| {
            do_poll(None, |table| {
                poll_wait(&queue, table);
                ready.load(Ordering::SeqCst).then_some("ready")
            })
        });
        let spent = started.elapsed();
        join("the producer", producer);
        assert!(
            answer == Ok(Some("ready"))
                && queue.waiters().is_empty()
                && spent < Duration::from_secs(5),
            "do_poll with no timeout over a queue another thread wakes once its event arrives \
             must answer what the poll then finds at once, and leave the queue, got {answer:?} \
             after {spent:?}"
        );
    }

    /// A task's wake, counted.
    #[derive(Default)]
    struct CountsWakes(std::sync::atomic::AtomicUsize);

    impl std::task::Wake for CountsWakes {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn a_task_waiting_on_a_queue_is_woken_once_by_its_next_wake() {
        let queue = WaitQueueHead::default();
        let wakes = Arc::new(CountsWakes::default());
        let waker = Waker::from(wakes.clone());
        queue.add(&waker);
        queue.add(&waker);
        queue.wake_up();
        queue.wake_up();
        assert_eq!(
            wakes.0.load(Ordering::SeqCst),
            1,
            "a task added to a wait queue twice must be woken once by the next wake_up and not \
             by a later one, as an autoremoving wait entry leaves once it wakes"
        );
    }

    // @behavior RT-335
    #[test]
    fn a_poll_finding_nothing_answers_none_once_its_timeout_passes() {
        let started = Instant::now();
        let answer = do_poll(Some(Duration::from_millis(20)), |_| None::<()>);
        assert!(
            answer == Ok(None) && started.elapsed() >= Duration::from_millis(20),
            "do_poll with a 20 ms timeout over nothing ready must answer None once it passes, \
             got {answer:?} after {:?}",
            started.elapsed()
        );
    }

    // @behavior RT-267
    #[test]
    fn a_poll_ends_with_eintr_once_its_thread_is_interrupted() {
        let (task, waiting) = (Arc::new(Mutex::new(None)), Arc::new(AtomicBool::new(false)));
        let poller = {
            let (task, waiting) = (task.clone(), waiting.clone());
            thread::spawn(move || {
                run_task(Some(Instant::now() + Duration::from_secs(10)), || {
                    *task.lock().unwrap() = Some(Task::current());
                    do_poll(None, |_| {
                        waiting.store(true, Ordering::SeqCst);
                        None::<()>
                    })
                })
            })
        };
        crate::sched::testing::wait_until("the poll", || waiting.load(Ordering::SeqCst));
        // The poller has asked once and gone to sleep by now, so only a wake
        // from the interrupt ends it before its deadline.
        thread::sleep(Duration::from_millis(50));
        task.lock().unwrap().as_ref().unwrap().interrupt();
        let started = Instant::now();
        let answer = join("the interrupted poll", poller);
        assert!(
            answer == Err(Errno::Interrupted) && started.elapsed() < Duration::from_secs(1),
            "do_poll with no timeout, its thread interrupted, must answer EINTR without waiting \
             for its deadline, got {answer:?} after {:?}",
            started.elapsed()
        );
    }

    // @behavior RT-267
    #[test]
    fn a_poll_ends_with_eintr_once_its_deadline_passes() {
        let answer = run_task(Some(Instant::now() + Duration::from_millis(20)), || {
            do_poll(None, |_| None::<()>)
        });
        assert_eq!(
            answer,
            Err(Errno::Interrupted),
            "do_poll with no timeout, past its run's deadline, must answer EINTR"
        );
    }

    // @behavior RT-336
    #[test]
    fn a_poll_that_sleeps_waits_through_the_registered_blocking() {
        let waits = count_waits();
        let _ = do_poll(Some(Duration::from_millis(1)), |_| None::<()>);
        let _ = do_poll(Some(Duration::from_secs(1)), |_| Some(()));
        assert_eq!(
            waits.load(Ordering::SeqCst),
            1,
            "do_poll must sleep through the registered Blocking only when its first ask finds \
             nothing ready"
        );
    }
}
