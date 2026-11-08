//! The connections a guest holds open, counted against its Sandbox and the
//! process, as a cgroup's `pids.max` bounds the tasks beneath it, so one
//! tenant cannot starve another of host sockets. A socket, an HTTP request,
//! and a name lookup in flight each hold one place until released.

use std::sync::{Arc, LazyLock, Mutex, PoisonError};

/// The process's limit until a Host App sets its own, as tokio caps its
/// blocking pool.
const PROCESS_DEFAULT: usize = 512;

static PROCESS: LazyLock<Arc<OpenConnections>> =
    LazyLock::new(|| OpenConnections::new(Some(PROCESS_DEFAULT)));

/// Every connection the process's Sandboxes hold open together, 512 at most
/// until `set_limit` moves it.
pub fn process_connections() -> &'static Arc<OpenConnections> {
    &PROCESS
}

/// How many connections one owner holds open, and the most it may. A count is
/// the process's own: a forked child, which inherits no socket of its
/// parent's runs, starts from none.
pub struct OpenConnections {
    /// The most held at once; `None` lifts the limit.
    limit: Mutex<Option<usize>>,
    /// The process the count was made in, and the count.
    count: Mutex<(u32, usize)>,
}

impl OpenConnections {
    /// None held yet, at most `limit` at once; `None` lifts the limit.
    pub fn new(limit: Option<usize>) -> Arc<Self> {
        Arc::new(Self {
            limit: Mutex::new(limit),
            count: Mutex::new((std::process::id(), 0)),
        })
    }

    /// Move the limit, as writing `pids.max` does: one held past a lowered
    /// limit stays open, and the next is refused.
    pub fn set_limit(&self, limit: Option<usize>) {
        *self.limit.lock().unwrap_or_else(PoisonError::into_inner) = limit;
    }

    /// Whether one more fits, counting it if so.
    fn try_open(&self) -> bool {
        let limit = *self.limit.lock().unwrap_or_else(PoisonError::into_inner);
        let mut count = self.count.lock().unwrap_or_else(PoisonError::into_inner);
        let pid = std::process::id();
        if count.0 != pid {
            *count = (pid, 0);
        }
        if limit.is_some_and(|limit| count.1 >= limit) {
            return false;
        }
        count.1 += 1;
        true
    }

    /// One given back, counted in `pid`.
    fn close(&self, pid: u32) {
        let mut count = self.count.lock().unwrap_or_else(PoisonError::into_inner);
        if count.0 == pid {
            count.1 -= 1;
        }
    }
}

/// One connection's place against its Sandbox and the process, given back as
/// it drops, as a task's exit uncharges its cgroup.
pub(crate) struct Place {
    owners: [Arc<OpenConnections>; 2],
    pid: u32,
}

impl Drop for Place {
    fn drop(&mut self) {
        self.owners.iter().for_each(|owner| owner.close(self.pid));
    }
}

/// A place counted against `sandbox` and the process, or `None` when either
/// is full.
pub(crate) fn open(sandbox: &Arc<OpenConnections>) -> Option<Place> {
    open_within(sandbox, &PROCESS)
}

/// A place counted against `sandbox` and then `process`, or `None`, counting
/// nothing, when either is full.
fn open_within(sandbox: &Arc<OpenConnections>, process: &Arc<OpenConnections>) -> Option<Place> {
    let pid = std::process::id();
    if !sandbox.try_open() {
        return None;
    }
    if !process.try_open() {
        sandbox.close(pid);
        return None;
    }
    Some(Place {
        owners: [sandbox.clone(), process.clone()],
        pid,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // @behavior NW-022
    #[test]
    fn a_full_sandbox_opens_no_more_until_one_is_given_back() {
        let sandbox = OpenConnections::new(Some(2));
        let held = [open(&sandbox), open(&sandbox)];
        let full = open(&sandbox).is_none();
        drop(held);
        assert!(
            full && open(&sandbox).is_some(),
            "a Sandbox at its open connection limit must open no more until one is given back"
        );
    }

    // @behavior NW-022
    #[test]
    fn a_full_process_opens_no_more_for_any_sandbox() {
        let process = OpenConnections::new(Some(2));
        let (first, second) = (OpenConnections::new(None), OpenConnections::new(None));
        let held = [open_within(&first, &process), open_within(&first, &process)];
        let refused = open_within(&second, &process).is_none();
        let counted_while_refused = second.count.lock().unwrap().1;
        drop(held);
        assert!(
            refused && counted_while_refused == 0 && open_within(&second, &process).is_some(),
            "a process at its limit must open no more for another Sandbox until one is given \
             back, counting nothing against the Sandbox it refused"
        );
    }

    // @behavior NW-022
    #[test]
    fn a_count_made_in_another_process_starts_from_none() {
        let sandbox = OpenConnections::new(Some(2));
        *sandbox.count.lock().unwrap() = (std::process::id().wrapping_add(1), 2);
        assert!(
            open(&sandbox).is_some(),
            "a connection count inherited across a fork must start from none in the child"
        );
    }

    // @behavior NW-060
    #[test]
    fn a_lowered_limit_keeps_what_is_open_and_refuses_the_next() {
        let process = OpenConnections::new(Some(3));
        let sandbox = OpenConnections::new(None);
        let held = [
            open_within(&sandbox, &process),
            open_within(&sandbox, &process),
        ];
        process.set_limit(Some(1));
        let refused = open_within(&sandbox, &process).is_none();
        process.set_limit(None);
        let lifted = open_within(&sandbox, &process).is_some();
        assert!(
            held.iter().all(Option::is_some) && refused && lifted,
            "OpenConnections::set_limit below what is open must keep each one open and refuse \
             the next, and None must lift the limit"
        );
    }
}
