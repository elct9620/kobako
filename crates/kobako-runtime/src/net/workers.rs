//! The worker threads doing a socket's host I/O, counted against their
//! Sandbox and the process, as a cgroup's `pids.max` bounds the tasks beneath
//! it, so one tenant cannot starve another of threads.

use std::sync::{Arc, LazyLock, Mutex, PoisonError};

/// The most worker threads one Sandbox runs at once: two per open socket.
pub const SANDBOX_WORKERS: usize = 64;

/// The most worker threads the process runs at once, across every Sandbox.
pub const PROCESS_WORKERS: usize = 1024;

static PROCESS: LazyLock<Arc<Workers>> = LazyLock::new(|| Workers::new(PROCESS_WORKERS));

/// How many workers one owner runs, and the most it may. A count is the
/// process's own: a forked child, which inherits no thread, starts from none.
pub struct Workers {
    limit: usize,
    /// The process the count was made in, and the count.
    count: Mutex<(u32, usize)>,
}

impl Workers {
    /// An owner running at most `limit` workers.
    pub fn new(limit: usize) -> Arc<Self> {
        Arc::new(Self {
            limit,
            count: Mutex::new((std::process::id(), 0)),
        })
    }

    /// Whether `n` more workers fit, counting them if so.
    fn try_charge(&self, n: usize) -> bool {
        let mut count = self.count.lock().unwrap_or_else(PoisonError::into_inner);
        let pid = std::process::id();
        if count.0 != pid {
            *count = (pid, 0);
        }
        if count.1 + n > self.limit {
            return false;
        }
        count.1 += n;
        true
    }

    /// One worker ended, counted in `pid`.
    fn uncharge(&self, pid: u32) {
        let mut count = self.count.lock().unwrap_or_else(PoisonError::into_inner);
        if count.0 == pid {
            count.1 -= 1;
        }
    }
}

/// One worker's place against its Sandbox and the process, given back as it
/// drops, as a task's exit uncharges its cgroup.
pub(crate) struct Charge {
    owners: [Arc<Workers>; 2],
    pid: u32,
}

impl Drop for Charge {
    fn drop(&mut self) {
        self.owners
            .iter()
            .for_each(|owner| owner.uncharge(self.pid));
    }
}

/// Places for `n` workers of `sandbox`, or `None` when it or the process is
/// full.
pub(crate) fn charge(sandbox: &Arc<Workers>, n: usize) -> Option<Vec<Charge>> {
    charge_within(sandbox, &PROCESS, n)
}

/// Places for `n` workers counted against `sandbox` and then `process`, or
/// `None`, counting nothing, when either is full.
fn charge_within(sandbox: &Arc<Workers>, process: &Arc<Workers>, n: usize) -> Option<Vec<Charge>> {
    let pid = std::process::id();
    if !sandbox.try_charge(n) {
        return None;
    }
    if !process.try_charge(n) {
        (0..n).for_each(|_| sandbox.uncharge(pid));
        return None;
    }
    Some(
        (0..n)
            .map(|_| Charge {
                owners: [sandbox.clone(), process.clone()],
                pid,
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // @behavior NW-022
    #[test]
    fn a_full_sandbox_takes_no_more_workers_until_one_ends() {
        let sandbox = Workers::new(2);
        let held = charge(&sandbox, 2).expect("two workers fit");
        let full = charge(&sandbox, 1).is_none();
        drop(held);
        assert!(
            full && charge(&sandbox, 2).is_some(),
            "a Sandbox at its worker cap must take no more until one ends, then take them again"
        );
    }

    // @behavior NW-022
    #[test]
    fn a_full_process_takes_no_more_workers_for_any_sandbox() {
        let process = Workers::new(2);
        let (first, second) = (Workers::new(2), Workers::new(2));
        let held = charge_within(&first, &process, 2).expect("two workers fit");
        let refused = charge_within(&second, &process, 1).is_none();
        drop(held);
        assert!(
            refused && charge_within(&second, &process, 2).is_some(),
            "a process at its worker cap must take no more for another Sandbox until one ends, \
             counting nothing against the Sandbox it refused"
        );
    }

    // @behavior NW-022
    #[test]
    fn a_count_made_in_another_process_starts_from_none() {
        let sandbox = Workers::new(2);
        *sandbox.count.lock().unwrap() = (std::process::id().wrapping_add(1), 2);
        assert!(
            charge(&sandbox, 2).is_some(),
            "a worker count inherited across a fork must start from none in the child"
        );
    }
}
