//! How many files one Invocation holds open across every table it has, as a
//! Linux process's `RLIMIT_NOFILE` counts each descriptor it holds. A socket
//! is a file too, so the preview1 descriptor table and preview2's resource
//! table share one count.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use super::Errno;

/// The files an Invocation holds, against its limit; each table holding one
/// shares the count.
#[derive(Clone, Debug, Default)]
pub struct OpenFiles {
    held: Arc<AtomicUsize>,
    limit: Option<usize>,
}

impl OpenFiles {
    /// No file held yet, at most `limit` at once; `None` lifts the limit.
    pub fn new(limit: Option<usize>) -> Self {
        Self {
            held: Arc::default(),
            limit,
        }
    }

    /// Whether one more would pass the limit.
    pub fn is_full(&self) -> bool {
        self.limit
            .is_some_and(|limit| self.held.load(Ordering::SeqCst) >= limit)
    }

    /// Hold one more, refused as EMFILE at the limit, as `get_unused_fd_flags`
    /// refuses one past `RLIMIT_NOFILE`.
    pub fn open(&self) -> Result<(), Errno> {
        match self.is_full() {
            true => Err(Errno::TooManyOpenFiles),
            false => {
                self.held.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        }
    }

    /// Give one back, as closing a descriptor does.
    pub fn close(&self) {
        self.held.fetch_sub(1, Ordering::SeqCst);
    }

    /// Hold one more, as `open` does, given back once what it returns drops.
    pub(crate) fn hold(&self) -> Result<HeldFile, Errno> {
        self.open()?;
        Ok(HeldFile(self.clone()))
    }
}

/// One file held against an Invocation's count, given back as it drops.
#[must_use = "the file is given back as soon as this drops"]
pub(crate) struct HeldFile(OpenFiles);

impl Drop for HeldFile {
    fn drop(&mut self) {
        self.0.close();
    }
}

/// The open a Filesystem answers on this thread: whose files it counts, and
/// whether the descriptor's own file still covers the next host descriptor
/// held behind it. The VFS sets it around `->open`, as Linux charges what an
/// open allocates to `current`.
struct Opening {
    files: OpenFiles,
    covered: bool,
}

thread_local! {
    static OPENING: std::cell::RefCell<Option<Opening>> = const { std::cell::RefCell::new(None) };
}

/// Run `open`, a Filesystem opening one descriptor for whoever `files`
/// counts, so each host descriptor it holds behind that descriptor is
/// counted there. What was set before comes back once it returns.
pub(crate) fn opening<T>(files: &OpenFiles, open: impl FnOnce() -> T) -> T {
    /// Puts back what was set before, however `open` ends.
    struct Restore(Option<Opening>);

    impl Drop for Restore {
        fn drop(&mut self) {
            let before = self.0.take();
            OPENING.with(|current| current.replace(before));
        }
    }

    let opened = Opening {
        files: files.clone(),
        covered: true,
    };
    let _restore = Restore(OPENING.with(|current| current.replace(Some(opened))));
    open()
}

/// Count one more host descriptor held behind the descriptor being opened:
/// the descriptor's own file covers the first, and each after it holds one
/// more, refused as EMFILE at the limit before the host is asked. Nothing is
/// counted outside an open, as a listing that opens and closes at once.
pub(crate) fn hold_behind() -> Result<Option<HeldFile>, Errno> {
    OPENING.with(|current| match current.borrow_mut().as_mut() {
        None => Ok(None),
        Some(opening) => match std::mem::take(&mut opening.covered) {
            true => Ok(None),
            false => opening.files.hold().map(Some),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_tables_sharing_the_count_meet_one_limit() {
        let preview1 = OpenFiles::new(Some(2));
        let preview2 = preview1.clone();
        let answers = (preview1.open(), preview2.open(), preview1.open());
        preview2.close();
        assert_eq!(
            (answers, preview1.open()),
            ((Ok(()), Ok(()), Err(Errno::TooManyOpenFiles)), Ok(())),
            "OpenFiles cloned for two tables must refuse the third file as EMFILE against a \
             limit of two, and take one again once either closes"
        );
    }
}
