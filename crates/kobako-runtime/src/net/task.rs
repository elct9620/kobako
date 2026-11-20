//! The work an HTTP exchange or a name lookup does, as a future only the
//! guest waiting on it polls, as a process moves its own non-blocking
//! requests only while it runs. A request's slow step, such as a certificate
//! check, so holds its own guest's thread and never another Sandbox's wait.
//! The reactor only wakes the request's wait queue.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::task::{Context, Wake, Waker};

use crate::wait::WaitQueueHead;

type Running = Pin<Box<dyn Future<Output = ()> + Send>>;

pub(crate) struct Task {
    future: Mutex<Option<Running>>,
    /// Wakes the queue the request's guest waits on, so it polls again.
    waker: Waker,
}

impl Task {
    /// `future`, each wake of which wakes `wait`.
    pub(crate) fn new(
        future: impl Future<Output = ()> + Send + 'static,
        wait: Arc<WaitQueueHead>,
    ) -> Self {
        Self {
            future: Mutex::new(Some(Box::pin(future))),
            waker: Waker::from(Arc::new(WakesQueue(wait))),
        }
    }

    /// Move the future as far as it goes without waiting, letting go of what
    /// it holds once it ends.
    pub(crate) fn poll(&self) {
        let mut future = self.future();
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
        let opener = {
            let gate = gate.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(20));
                let waker = {
                    let mut gate = gate.lock().unwrap();
                    gate.0 = true;
                    gate.1.take()
                };
                waker.expect("the task waited").wake();
            })
        };
        let answer = do_poll(
            || Some(std::time::Instant::now() + std::time::Duration::from_secs(5)),
            |table| {
                poll_wait(&wait, table);
                task.poll();
                ended.load(Ordering::SeqCst).then_some(())
            },
        );
        opener.join().unwrap();
        assert_eq!(
            answer,
            Ok(Some(())),
            "a request woken from another thread must wake the queue its guest waits on, and \
             end once the guest polls it again"
        );
    }
}
