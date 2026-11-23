//! The host directories Binds keep open, each a descriptor of the host's own.
//! A dentry holds the one above it, so keeping one for every dentry would
//! spend a descriptor per level of depth, which no table of a guest's counts.
//! A directory the guest holds open keeps its own; the rest are kept up to a
//! bound the process's Binds share, the oldest let go and walked to again
//! when asked, as 9p walks to a dentry it holds no fid for.
//!
//! What is kept is only the room no one else took: a host out of descriptors
//! makes every kept directory give way before an open is tried once more, as
//! Linux's `dcache` shrinks under a full `files_stat`. Each is let go one at
//! a time and closed past the lock, so no call waits on room and none holds
//! the lock across the host.

use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex, PoisonError, Weak};

use cap_std::fs::Dir;
use lru::LruCache;

use crate::fork::ProcessLocal;
use crate::vfs::Dentry;
use crate::Errno;

/// The most directories the process's Binds keep open for walks.
const MAX_CACHED_FIDS: NonZeroUsize = NonZeroUsize::new(64).expect("the bound is not zero");

/// A directory, the Bind it was walked to through, and the dentry naming it.
struct CachedFid {
    owner: usize,
    _dentry: Weak<Dentry>,
    dir: Arc<Dir>,
}

/// What the process keeps, by the address of the dentry naming each; each
/// entry holds that dentry's allocation, so no other can take its address.
/// While a call gives way, nothing more is kept.
struct Kept {
    fids: LruCache<usize, CachedFid>,
    yielding: usize,
}

impl Default for Kept {
    fn default() -> Self {
        Self {
            fids: LruCache::unbounded(),
            yielding: 0,
        }
    }
}

/// The directories one process keeps for walks. A forked child keeps none of
/// its parent's: it starts from its own, and never takes the parent's lock.
pub(super) struct FidCache(ProcessLocal<Kept>);

/// The one every Bind keeps its directories in.
pub(super) static FIDS: FidCache = FidCache::new();

fn key(dentry: &Dentry) -> usize {
    std::ptr::from_ref(dentry) as usize
}

impl FidCache {
    pub(super) const fn new() -> Self {
        Self(ProcessLocal::new())
    }

    fn with<R>(&self, f: impl FnOnce(&mut Kept) -> R) -> R {
        self.0
            .with(|kept| f(kept.get_or_insert_with(Kept::default)))
    }

    /// The directory kept for `dentry`, now the most recently asked.
    pub(super) fn get(&self, dentry: &Dentry) -> Option<Arc<Dir>> {
        self.with(|kept| kept.fids.get(&key(dentry)).map(|fid| fid.dir.clone()))
    }

    /// Keep `dir` for `dentry`, walked to through `owner`, unless the bound is
    /// reached or a call gives way; what is not kept closes at its last
    /// reference, past the lock.
    pub(super) fn put(&self, owner: &FidOwner, dentry: &Dentry, dir: &Arc<Dir>) {
        let fid = CachedFid {
            owner: owner.id(),
            _dentry: Arc::downgrade(&dentry.get()),
            dir: dir.clone(),
        };
        let replaced =
            self.with(
                |kept| match kept.yielding == 0 && kept.fids.len() < MAX_CACHED_FIDS.get() {
                    true => kept.fids.put(key(dentry), fid),
                    false => Some(fid),
                },
            );
        drop(replaced);
    }

    /// Make room for one more, letting the oldest go when the bound is
    /// reached, so a walk's own keep never closes another's.
    pub(super) fn make_room(&self) {
        let oldest = self.with(|kept| match kept.fids.len() < MAX_CACHED_FIDS.get() {
            true => None,
            false => kept.fids.pop_lru(),
        });
        drop(oldest);
    }

    /// Open through `open`, and when the host is out of descriptors, let
    /// every kept directory go and try once more. One still in use stays open
    /// with whoever holds it, so only the room no one uses is given back.
    pub(super) fn open<T>(&self, mut open: impl FnMut() -> Result<T, Errno>) -> Result<T, Errno> {
        match open() {
            Err(Errno::TooManyOpenFiles | Errno::TooManyOpenFilesInSystem) => {
                let _yielding = Yielding::start(self);
                while let Some(fid) = self.with(|kept| kept.fids.pop_lru()) {
                    drop(fid);
                }
                open()
            }
            answer => answer,
        }
    }

    /// Let every directory kept through `owner` go, one at a time.
    fn release(&self, owner: usize) {
        let next = || {
            self.with(|kept| {
                let found = kept.fids.iter().find(|(_, fid)| fid.owner == owner);
                let key = found.map(|(key, _)| *key)?;
                kept.fids.pop(&key)
            })
        };
        while let Some(fid) = next() {
            drop(fid);
        }
    }

    /// How many directories are kept.
    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.with(|kept| kept.fids.len())
    }

    /// How many directories are kept through the owner `id` names, which
    /// may since have been let go.
    #[cfg(test)]
    pub(super) fn kept_by(&self, id: usize) -> usize {
        self.with(|kept| kept.fids.iter().filter(|(_, fid)| fid.owner == id).count())
    }

    /// Stand in for a fork: what is kept now was kept by another process.
    #[cfg(test)]
    pub(super) fn inherit(&self) {
        self.0.inherit();
    }
}

/// A call giving way, from its first letting go until its open was tried once
/// more, however that try ends.
struct Yielding<'a>(&'a FidCache);

impl<'a> Yielding<'a> {
    fn start(cache: &'a FidCache) -> Self {
        cache.with(|kept| kept.yielding += 1);
        Self(cache)
    }
}

impl Drop for Yielding<'_> {
    fn drop(&mut self) {
        self.0.with(|kept| kept.yielding -= 1);
    }
}

/// What one Bind's walks keep, let go with the last of its clones, named by
/// the address of the allocation the Bind shares it through. The Bind drops
/// its root before this, as the root is its maker's, not the cache's.
#[derive(Default)]
pub(super) struct FidOwner;

impl FidOwner {
    pub(super) fn id(&self) -> usize {
        std::ptr::from_ref(self) as usize
    }
}

impl Drop for FidOwner {
    fn drop(&mut self) {
        FIDS.release(self.id());
    }
}

/// The host directory a guest holds open on a dentry, kept on it for as long
/// as the guest holds it, as 9p keeps a fid in `d_fsdata`.
#[derive(Default)]
pub(super) struct OpenDirFid(Mutex<Weak<Dir>>);

impl OpenDirFid {
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

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use cap_std::ambient_authority;

    use super::*;
    use crate::filesystem::Tmpfs;
    use crate::vfs::SuperBlock;

    /// A host directory, open as each kept one is.
    fn host_dir() -> Arc<Dir> {
        Arc::new(
            Dir::open_ambient_dir(std::env::temp_dir(), ambient_authority())
                .expect("the temporary directory must open"),
        )
    }

    /// A cache of its own holding one directory, so no other test's walks
    /// reach what this one counts.
    fn holding_one(owner: &FidOwner, sb: &SuperBlock) -> (FidCache, Arc<Dentry>) {
        let cache = FidCache::new();
        let dentry = sb.dentries().alloc_pseudo();
        cache.put(owner, &dentry, &host_dir());
        (cache, dentry)
    }

    // @behavior FS-217
    #[test]
    fn a_host_out_of_descriptors_makes_the_kept_directories_give_way() {
        let sb = SuperBlock::new(Tmpfs::new()).expect("the instance must stand");
        let owner = FidOwner;
        let (cache, _dentry) = holding_one(&owner, &sb);
        let tries = Cell::new(0);
        let answer = cache.open(|| {
            tries.set(tries.get() + 1);
            match tries.get() {
                1 => Err(Errno::TooManyOpenFiles),
                _ => Ok(cache.len()),
            }
        });
        assert_eq!(
            (answer, tries.get()),
            (Ok(0), 2),
            "FidCache::open refused as EMFILE must let every kept directory go and try once more"
        );
    }

    // @behavior FS-218
    #[test]
    fn an_open_refused_again_after_giving_way_answers_the_refusal() {
        let tries = Cell::new(0);
        let answer = FidCache::new().open(|| {
            tries.set(tries.get() + 1);
            Err::<(), _>(Errno::TooManyOpenFilesInSystem)
        });
        assert_eq!(
            (answer, tries.get()),
            (Err(Errno::TooManyOpenFilesInSystem), 2),
            "FidCache::open refused as ENFILE twice must answer ENFILE after its one more try"
        );
    }

    // @behavior FS-219
    #[test]
    fn nothing_is_kept_while_a_call_gives_way() {
        let sb = SuperBlock::new(Tmpfs::new()).expect("the instance must stand");
        let owner = FidOwner;
        let (cache, dentry) = holding_one(&owner, &sb);
        let mut first = true;
        let answer = cache.open(|| match std::mem::take(&mut first) {
            true => Err(Errno::TooManyOpenFiles),
            false => {
                cache.put(&owner, &dentry, &host_dir());
                Ok(())
            }
        });
        assert_eq!(
            (answer, cache.len()),
            (Ok(()), 0),
            "a FidCache::put during FidCache::open giving way must keep nothing"
        );
    }

    // @behavior FS-222
    #[test]
    fn a_forked_child_keeps_none_of_its_parents_directories() {
        let sb = SuperBlock::new(Tmpfs::new()).expect("the instance must stand");
        let owner = FidOwner;
        let (cache, dentry) = holding_one(&owner, &sb);
        cache.inherit();
        assert_eq!(
            (cache.len(), cache.get(&dentry).is_none()),
            (0, true),
            "a FidCache a forked child inherits must keep none of its parent's directories"
        );
    }
}
