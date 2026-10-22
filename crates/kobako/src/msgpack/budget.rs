//! The value budget a guest payload is held to while one invocation runs
//! on this thread: one value for each 16 bytes of the memory cap, so what a
//! guest makes the host build stays in proportion to what it could hold.
//!
//! A thread-local rather than a parameter, because the positions that
//! decode a guest payload are reached through `Receiver` and `Yielder`,
//! whose signatures name no invocation setting. An invocation is
//! synchronous on the thread that drives it, and a nested one puts the
//! outer budget back when it returns.

use std::cell::Cell;

thread_local! {
    static LIMIT: Cell<Option<usize>> = const { Cell::new(None) };
}

/// Run `run` with every guest payload decoded on this thread held to
/// `limit` values, putting the outer budget back however `run` returns.
pub(crate) fn scope<T>(limit: Option<usize>, run: impl FnOnce() -> T) -> T {
    struct Restore(Option<usize>);
    impl Drop for Restore {
        fn drop(&mut self) {
            LIMIT.with(|cell| cell.set(self.0));
        }
    }
    let _restore = Restore(LIMIT.with(|cell| cell.replace(limit)));
    run()
}

/// The budget of the invocation this thread is driving, if any.
pub(crate) fn current() -> Option<usize> {
    LIMIT.with(Cell::get)
}
