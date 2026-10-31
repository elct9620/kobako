//! The host directories a Bind keeps open, each a descriptor of the host's
//! own. A dentry holds the one above it, so keeping one for every dentry would
//! spend a descriptor per level of depth, which no table of a guest's counts.
//! A directory the guest holds open keeps its own; the rest are kept up to a
//! bound, the oldest let go and walked to again when asked, as 9p walks to a
//! dentry it holds no fid for.

use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

use cap_std::fs::Dir;
use lru::LruCache;

use crate::vfs::Dentry;

/// The most directories a Bind keeps open for walks.
const WALKED_MAX: NonZeroUsize = NonZeroUsize::new(64).expect("the bound is not zero");

/// Directories a Bind walked to, by the address of the dentry naming each.
/// Each entry holds that dentry's allocation, so no other can take its address.
pub(super) struct Walked(Mutex<LruCache<usize, Kept>>);

/// A directory, and the dentry it was walked to for.
type Kept = (Weak<Dentry>, Arc<Dir>);

impl Default for Walked {
    fn default() -> Self {
        Self(Mutex::new(LruCache::new(WALKED_MAX)))
    }
}

fn key(dentry: &Dentry) -> usize {
    std::ptr::from_ref(dentry) as usize
}

impl Walked {
    fn entries(&self) -> MutexGuard<'_, LruCache<usize, Kept>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The directory kept for `dentry`, now the most recently asked.
    pub(super) fn get(&self, dentry: &Dentry) -> Option<Arc<Dir>> {
        self.entries().get(&key(dentry)).map(|(_, dir)| dir.clone())
    }

    /// Keep `dir` for `dentry`, letting go of the oldest past the bound.
    pub(super) fn keep(&self, dentry: &Dentry, dir: &Arc<Dir>) {
        let entry = (Arc::downgrade(&dentry.get()), dir.clone());
        self.entries().put(key(dentry), entry);
    }

    /// How many directories are kept.
    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.entries().len()
    }
}

/// The host directory a guest holds open on a dentry, kept on it for as long
/// as the guest holds it, as 9p keeps a fid in `d_fsdata`.
#[derive(Default)]
pub(super) struct Opened(Mutex<Weak<Dir>>);

impl Opened {
    /// The directory on `dentry`, while a guest holds it open.
    pub(super) fn get(dentry: &Dentry) -> Option<Arc<Dir>> {
        let kept = dentry.fsdata().get()?.clone().downcast::<Self>().ok()?;
        let dir = kept.0.lock().unwrap_or_else(PoisonError::into_inner);
        dir.upgrade()
    }

    /// `dir` is held open on `dentry` from now on.
    pub(super) fn set(dentry: &Dentry, dir: &Arc<Dir>) {
        let kept = dentry
            .fsdata()
            .get_or_init(|| Arc::new(Self::default()))
            .clone();
        if let Ok(kept) = kept.downcast::<Self>() {
            *kept.0.lock().unwrap_or_else(PoisonError::into_inner) = Arc::downgrade(dir);
        }
    }
}
