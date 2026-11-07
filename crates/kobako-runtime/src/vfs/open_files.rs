//! How many files one Invocation holds open across every table it has, as a
//! Linux process's `RLIMIT_NOFILE` counts each descriptor it holds. A socket
//! is a file too, so the preview1 descriptor table and preview2's resource
//! table share one count.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use super::FilesystemError;

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
    pub fn open(&self) -> Result<(), FilesystemError> {
        match self.is_full() {
            true => Err(FilesystemError::TooManyOpenFiles),
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
            (
                (Ok(()), Ok(()), Err(FilesystemError::TooManyOpenFiles)),
                Ok(())
            ),
            "OpenFiles cloned for two tables must refuse the third file as EMFILE against a \
             limit of two, and take one again once either closes"
        );
    }
}
