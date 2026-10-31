//! The entries of an Overlay's view as it keeps them on their inodes, as
//! overlayfs keeps an `ovl_inode` for each: a directory's Layers'
//! directories, the lock a copy up of the entry holds, and where a file's
//! data is.
//!
//! The runtime records one inode for each dentry it reaches, so every handle
//! of an entry reaches the same state, and a removal or a replacement
//! through the runtime leaves the next lookup an inode, and so a state, of
//! its own.

use std::any::Any;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use super::super::FilesystemError;
use super::path::OverlayPath;
use super::stack::Level;
use crate::locking::RwSemaphore;
use crate::vfs::{Dentry, Inode};

/// One entry of the view.
pub(super) struct OverlayInode {
    /// Each Layer's directory a directory stands for; empty for a file.
    level: Mutex<Level>,
    /// The lock a copy up of the entry holds, as overlayfs's
    /// `ovl_inode_lock`, so two calls never copy one entry up at once.
    pub(super) lock: Arc<RwSemaphore>,
    /// The Layer's dentry a lookup found this file at, or a copy up made,
    /// as `struct ovl_entry` holds the real dentries, so the entry answers
    /// from it even once its name is gone from the view.
    real: Mutex<Option<Arc<Dentry>>>,
}

fn lock<T>(held: &Mutex<T>) -> MutexGuard<'_, T> {
    held.lock().unwrap_or_else(PoisonError::into_inner)
}

impl OverlayInode {
    fn new(level: Level, real: Option<Arc<Dentry>>) -> Self {
        Self {
            level: Mutex::new(level),
            lock: Arc::default(),
            real: Mutex::new(real),
        }
    }

    pub(super) fn level(&self) -> Level {
        lock(&self.level).clone()
    }

    /// Layer `into` now holds `dir` as this directory, made or found there
    /// by a copy up, as overlayfs's `ovl_inode_update` records the upper
    /// dentry, so every handle of it asks that Layer too.
    pub(super) fn inode_update(&self, into: usize, dir: &OverlayPath) {
        lock(&self.level)[into] = Some(dir.clone());
    }

    /// Layer `into` now holds `dir` as this directory, marked opaque, so no
    /// Layer below shows here any more.
    pub(super) fn inode_replace(&self, into: usize, dir: &OverlayPath) {
        let mut level = lock(&self.level);
        level.fill(None);
        level[into] = Some(dir.clone());
    }

    /// The Layer's dentry this file answers through.
    pub(super) fn real(&self) -> Option<Arc<Dentry>> {
        lock(&self.real).clone()
    }

    /// This file's data now stands at `real`, in the upper, as
    /// `ovl_dentry_set_upper_alias` records a copy up.
    pub(super) fn set_real(&self, real: Arc<Dentry>) {
        *lock(&self.real) = Some(real);
    }
}

/// The state the entry `dentry` records keeps, made from the level `init`
/// finds where it keeps none yet, as overlayfs's `ovl_get_inode` finds or
/// makes one. A dentry recording nothing names no entry to keep it for.
pub(super) fn ovl_inode(
    dentry: &Dentry,
    init: impl FnOnce() -> Result<Level, FilesystemError>,
) -> Result<Arc<OverlayInode>, FilesystemError> {
    let inode = dentry.inode().ok_or(FilesystemError::NotFound)?;
    if let Some(kept) = inode.private().get() {
        return Ok(downcast(kept.clone()));
    }
    let made: Arc<dyn Any + Send + Sync> = Arc::new(OverlayInode::new(init()?, None));
    Ok(downcast(inode.private().get_or_init(|| made).clone()))
}

/// Whether the entry `dentry` records keeps its state already.
pub(super) fn ovl_inode_is_kept(dentry: &Dentry) -> bool {
    dentry
        .inode()
        .is_some_and(|inode| inode.private().get().is_some())
}

/// Keep on `inode` the Layer's dentry its lookup found, as `ovl_lookup` keeps
/// each real dentry.
pub(super) fn ovl_inode_real(inode: &Inode, real: Arc<Dentry>) {
    inode
        .private()
        .get_or_init(|| Arc::new(OverlayInode::new(Vec::new(), Some(real))));
}

/// The state `inode` keeps, where it keeps any.
pub(super) fn ovl_inode_of(inode: &Inode) -> Option<Arc<OverlayInode>> {
    Some(downcast(inode.private().get()?.clone()))
}

fn downcast(kept: Arc<dyn Any + Send + Sync>) -> Arc<OverlayInode> {
    kept.downcast()
        .expect("an Overlay's inode keeps an Overlay's state")
}
