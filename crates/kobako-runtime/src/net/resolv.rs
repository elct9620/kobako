//! The host's own resolver, asked so a lookup can be cancelled: each platform
//! answers through the query its system hands out to be waited on and
//! dropped, so no lookup holds a thread that outlives it, as glibc's
//! `getaddrinfo` would.

#[cfg(target_os = "macos")]
mod darwin;
#[cfg(all(unix, not(target_os = "macos")))]
mod stub;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
use std::sync::{Mutex, PoisonError};
#[cfg(unix)]
use std::task::{Context, Poll, Waker};

use super::lookup::Resolving;
#[cfg(unix)]
use super::reactor::Ready;

/// The host's addresses for `name`, a fully qualified name with its root.
pub(super) fn resolve(name: &str) -> Resolving {
    #[cfg(target_os = "macos")]
    return darwin::resolve(name);
    #[cfg(windows)]
    return windows::resolve(name);
    #[cfg(all(unix, not(target_os = "macos")))]
    return stub::resolve(name);
}

/// Wakes the task waiting on a resolver's socket, keeping a change that came
/// while no task waited, so a task that reads after it learns of it.
#[cfg(unix)]
#[derive(Default)]
struct ReadyWaker(Mutex<(bool, Option<Waker>)>);

#[cfg(unix)]
impl ReadyWaker {
    /// Ready once the socket changed since the last ready answer; otherwise
    /// the task waits for the next change.
    fn poll_changed(&self, cx: &mut Context<'_>) -> Poll<()> {
        let mut state = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if std::mem::take(&mut state.0) {
            return Poll::Ready(());
        }
        state.1 = Some(cx.waker().clone());
        Poll::Pending
    }
}

#[cfg(unix)]
impl Ready for ReadyWaker {
    fn on_event(&self) {
        let mut state = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        state.0 = true;
        if let Some(waker) = state.1.take() {
            waker.wake();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::io;
    use std::net::IpAddr;
    use std::sync::Arc;
    use std::task::{Context, Poll, Wake};
    use std::thread::{self, Thread};
    use std::time::{Duration, Instant};

    use super::resolve;

    /// Wakes the thread that polls.
    struct Unparks(Thread);

    impl Wake for Unparks {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }

    /// What `future` ends with, or `None` once ten seconds pass first.
    pub(super) fn wait_for<F: Future>(future: F) -> Option<F::Output> {
        let mut future = std::pin::pin!(future);
        let waker = Arc::new(Unparks(thread::current())).into();
        let mut cx = Context::from_waker(&waker);
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Poll::Ready(output) = future.as_mut().poll(&mut cx) {
                return Some(output);
            }
            let left = deadline.checked_duration_since(Instant::now())?;
            thread::park_timeout(left.min(Duration::from_millis(50)));
        }
    }

    /// `name`'s answer, or `None` once ten seconds pass unanswered.
    fn answer_of(name: &str) -> Option<io::Result<Vec<IpAddr>>> {
        wait_for(resolve(name))
    }

    #[test]
    fn localhost_resolves_to_a_loopback_address() {
        let answer = answer_of("localhost.").map(|found| found.ok());
        assert!(
            matches!(&answer, Some(Some(found)) if !found.is_empty() && found.iter().all(IpAddr::is_loopback)),
            "the host's resolver must answer localhost with loopback addresses only, got {answer:?}"
        );
    }

    #[test]
    fn a_name_under_invalid_answers_no_address() {
        let answer = answer_of("kobako.invalid.");
        assert!(
            matches!(&answer, Some(Ok(found)) if found.is_empty())
                || matches!(answer, Some(Err(_))),
            "the host's resolver must answer a name under .invalid with no address, and answer \
             at all, got {answer:?}"
        );
    }

    #[test]
    fn a_query_dropped_before_its_answer_is_cancelled_cleanly() {
        drop(resolve("kobako-dropped.invalid."));
        assert!(
            answer_of("localhost.").is_some(),
            "the host's resolver must keep answering after a query was dropped unanswered"
        );
    }
}
