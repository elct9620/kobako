//! The work an HTTP exchange or a name lookup does, as a future only the
//! guest waiting on it polls, as a process moves its own non-blocking
//! requests only while it runs. A request's slow step, such as a certificate
//! check, so holds its own guest's thread and never another Sandbox's wait.
//! The reactor only wakes the request's wait queue.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::task::{Context, Poll, Wake, Waker};

use crate::wait::WaitQueueHead;

type Running = Pin<Box<dyn Future<Output = ()> + Send>>;

/// The steps one poll of a task may take on its resources, as tokio's
/// cooperative budget gives each poll of a task 128.
const STEPS_PER_POLL: usize = 128;

/// What one poll of a task may still move, as tokio's coop budget: each step
/// a resource takes spends one, and a resource that finds none left wakes
/// the task and answers `Pending`. A poll so returns however ready its
/// resources stay, and its guest looks at its deadline and its stop before
/// the next.
pub(crate) struct PollBudget(AtomicUsize);

impl Default for PollBudget {
    fn default() -> Self {
        Self(AtomicUsize::new(STEPS_PER_POLL))
    }
}

impl PollBudget {
    fn renew(&self) {
        self.0.store(STEPS_PER_POLL, Ordering::Relaxed);
    }

    /// A step to take, or `Pending` with the task woken once none is left,
    /// as `poll_proceed`; the step is given back unless it made progress.
    pub(crate) fn poll_proceed(&self, cx: &mut Context<'_>) -> Poll<Step<'_>> {
        let spent = self
            .0
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |left| {
                left.checked_sub(1)
            });
        match spent {
            Ok(_) => Poll::Ready(Step {
                budget: self,
                progressed: false,
            }),
            Err(_) => {
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        }
    }
}

/// One step a resource took, given back as it drops unless it made
/// progress, as tokio's `RestoreOnPending`.
pub(crate) struct Step<'a> {
    budget: &'a PollBudget,
    progressed: bool,
}

impl Step<'_> {
    pub(crate) fn made_progress(mut self) {
        self.progressed = true;
    }
}

impl Drop for Step<'_> {
    fn drop(&mut self) {
        if !self.progressed {
            self.budget.0.fetch_add(1, Ordering::Relaxed);
        }
    }
}

pub(crate) struct Task {
    future: Mutex<Option<Running>>,
    /// Wakes the queue the request's guest waits on, so it polls again.
    waker: Waker,
    /// Renewed for each poll, so one poll moves the future a bounded way.
    budget: Arc<PollBudget>,
}

impl Task {
    /// `future`, each wake of which wakes `wait`.
    pub(crate) fn new(
        future: impl Future<Output = ()> + Send + 'static,
        wait: Arc<WaitQueueHead>,
    ) -> Self {
        Self::spending(future, wait, Arc::default())
    }

    /// `future`, whose resources spend `budget`, renewed for each poll.
    pub(crate) fn spending(
        future: impl Future<Output = ()> + Send + 'static,
        wait: Arc<WaitQueueHead>,
        budget: Arc<PollBudget>,
    ) -> Self {
        Self {
            future: Mutex::new(Some(Box::pin(future))),
            waker: Waker::from(Arc::new(WakesQueue(wait))),
            budget,
        }
    }

    /// Move the future as far as its budget lets it without waiting, letting
    /// go of what it holds once it ends.
    pub(crate) fn poll(&self) {
        let mut future = self.future();
        self.budget.renew();
        let ended = future.as_mut().is_some_and(|running| {
            running
                .as_mut()
                .poll(&mut Context::from_waker(&self.waker))
                .is_ready()
        });
        if ended {
            *future = None;
        }
    }

    /// Drop the future where it stands, with every socket it holds.
    pub(crate) fn cancel(&self) {
        let future = self.future().take();
        drop(future);
    }

    fn future(&self) -> MutexGuard<'_, Option<Running>> {
        self.future.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

struct WakesQueue(Arc<WaitQueueHead>);

impl Wake for WakesQueue {
    fn wake(self: Arc<Self>) {
        self.0.wake_up();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.0.wake_up();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::task::Poll;

    use super::*;
    use crate::sched::testing::{count_waits, join, wait_until, within_max_wait};
    use crate::wait::{do_poll, poll_wait};

    // A request woken from another thread moves on once its guest polls it
    // again, as its wake reaches the queue the guest waits on.
    #[test]
    fn a_task_woken_from_another_thread_moves_once_its_guest_polls_again() {
        let wait = Arc::new(WaitQueueHead::default());
        let gate = Arc::new(Mutex::new((false, None::<Waker>)));
        let ended = Arc::new(AtomicBool::new(false));
        let task = {
            let (gate, ended) = (gate.clone(), ended.clone());
            Task::new(
                async move {
                    std::future::poll_fn(|cx| {
                        let mut gate = gate.lock().unwrap();
                        if gate.0 {
                            Poll::Ready(())
                        } else {
                            gate.1 = Some(cx.waker().clone());
                            Poll::Pending
                        }
                    })
                    .await;
                    ended.store(true, Ordering::SeqCst);
                },
                wait.clone(),
            )
        };
        task.poll();
        let waits = count_waits();
        let opener = {
            let (gate, waits) = (gate.clone(), waits.clone());
            std::thread::spawn(move || {
                wait_until("the guest waiting on the request's queue", || {
                    waits.load(Ordering::SeqCst) > 0
                });
                let waker = {
                    let mut gate = gate.lock().unwrap();
                    gate.0 = true;
                    gate.1.take()
                };
                waker.expect("the task waited").wake();
            })
        };
        let answer = within_max_wait(|| {
            do_poll(
                || None,
                |table| {
                    poll_wait(&wait, table);
                    // A poll once the deadline passed was not the wake's.
                    if crate::sched::fatal_signal_pending() {
                        return None;
                    }
                    task.poll();
                    ended.load(Ordering::SeqCst).then_some(())
                },
            )
        });
        join("the opener", opener);
        assert_eq!(
            answer,
            Ok(Some(())),
            "a request woken from another thread must wake the queue its guest waits on, and \
             end once the guest polls it again, before the guest's deadline ends its wait"
        );
    }
}
