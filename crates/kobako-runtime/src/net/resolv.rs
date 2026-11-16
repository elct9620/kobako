//! The host's own resolver, asked so a lookup can be cancelled: each platform
//! answers through the query its system hands out to be waited on and
//! dropped, so no lookup holds a thread that outlives it, as glibc's
//! `getaddrinfo` would.

#[cfg(all(unix, not(target_os = "macos")))]
mod blocking;
#[cfg(target_os = "macos")]
mod darwin;
#[cfg(windows)]
mod windows;

use super::lookup::Resolving;

/// The host's addresses for `name`, a fully qualified name with its root.
pub(super) fn resolve(name: &str) -> Resolving {
    #[cfg(target_os = "macos")]
    return darwin::resolve(name);
    #[cfg(windows)]
    return windows::resolve(name);
    #[cfg(all(unix, not(target_os = "macos")))]
    return blocking::resolve(name);
}

#[cfg(test)]
mod tests {
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

    /// `name`'s answer, or `None` once ten seconds pass unanswered.
    fn answer_of(name: &str) -> Option<io::Result<Vec<IpAddr>>> {
        let mut resolving = resolve(name);
        let waker = Arc::new(Unparks(thread::current())).into();
        let mut cx = Context::from_waker(&waker);
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Poll::Ready(answer) = resolving.as_mut().poll(&mut cx) {
                return Some(answer);
            }
            let left = deadline.checked_duration_since(Instant::now())?;
            thread::park_timeout(left.min(Duration::from_millis(50)));
        }
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
