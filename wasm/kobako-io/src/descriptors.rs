//! The descriptors an `IO` may stand for: the two captured Channels, and
//! each file this guest opened and has not yet closed.
//!
//! The record lives in guest memory, which every invocation starts from
//! the baked image, so no invocation inherits another's. A copy of an
//! `IO` keeps its descriptor after the original closes it, so each read
//! and write checks it here again.

use std::collections::BTreeSet;
use std::sync::Mutex;

static OPENED: Mutex<BTreeSet<i32>> = Mutex::new(BTreeSet::new());

fn opened() -> std::sync::MutexGuard<'static, BTreeSet<i32>> {
    OPENED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Descriptor 1 or 2, which the host captures.
pub(crate) fn is_channel(fd: i32) -> bool {
    fd == 1 || fd == 2
}

pub(crate) fn is_reachable(fd: i32) -> bool {
    is_channel(fd) || opened().contains(&fd)
}

pub(crate) fn remember(fd: i32) {
    opened().insert(fd);
}

/// Whether `fd` was one this guest opened, forgetting it either way.
pub(crate) fn forget(fd: i32) -> bool {
    opened().remove(&fd)
}
