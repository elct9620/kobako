//! The entries of an Overlay's view as it keeps them on their inodes, as
//! overlayfs keeps an `ovl_inode` for each: a directory's Layers' directories, or
//! where anything else stands in a Layer, with the lock a copy up of the entry
//! holds. A directory and a file keep apart what `ovl_inode` holds together, so
//! each answers only what it has.
//!
//! The runtime records one inode for each dentry it reaches, so every handle
//! of an entry reaches the same state, and a removal or a replacement
//! through the runtime leaves the next lookup an inode, and so a state, of
//! its own.

use std::any::Any;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use super::path::OverlayPath;
use super::stack::{Level, UPPER};
use crate::locking::RwSemaphore;
use crate::vfs::{Dentry, Inode};

/// A directory of the view.
pub(super) struct OverlayDirInode {
    /// Each Layer's directory it stands for.
    level: Mutex<Level>,
    /// The lock a copy up of it holds, as overlayfs's `ovl_inode_lock`, so two
    /// calls never copy it up at once.
    pub(super) lock: Arc<RwSemaphore>,
}

/// A file of the view, or anything else that is no directory.
pub(super) struct OverlayFileInode {
    /// The Layer and its dentry a lookup found it at, or a copy up made, as
    /// `struct ovl_entry` holds each real dentry with its layer, so it answers
    /// from there even once its name is gone from the view.
    real: Mutex<OverlayPath>,
    /// The lock a copy up of it holds, as overlayfs's `ovl_inode_lock`.
    pub(super) lock: Arc<RwSemaphore>,
}

fn lock<T>(held: &Mutex<T>) -> MutexGuard<'_, T> {
    held.lock().unwrap_or_else(PoisonError::into_inner)
}

impl OverlayDirInode {
    /// A directory standing for `level`, as `ovl_inode_params` carries it.
    pub(super) fn new(level: Level) -> Self {
        Self {
            level: Mutex::new(level),
            lock: Arc::default(),
        }
    }

    /// What `inode` keeps, where it is a directory's.
    pub(super) fn of(inode: &Inode) -> Option<Arc<Self>> {
        inode.private().get()?.clone().downcast().ok()
    }

    pub(super) fn level(&self) -> Level {
        lock(&self.level).clone()
    }

    /// The upper now holds `dir` as this directory, made or found there by a
    /// copy up, as overlayfs's `ovl_inode_update` records the upper dentry, so
    /// every handle of it asks the upper too.
    pub(super) fn inode_update(&self, dir: &OverlayPath) {
        lock(&self.level)[UPPER] = Some(dir.clone());
    }
}

impl OverlayFileInode {
    /// An entry answering through `real`, as `ovl_inode_params` carries it.
    pub(super) fn new(real: OverlayPath) -> Self {
        Self {
            real: Mutex::new(real),
            lock: Arc::default(),
        }
    }

    /// What `inode` keeps, where it is no directory's.
    pub(super) fn of(inode: &Inode) -> Option<Arc<Self>> {
        inode.private().get()?.clone().downcast().ok()
    }

    /// The Layer's dentry this answers through.
    pub(super) fn real(&self) -> Arc<Dentry> {
        lock(&self.real).dentry().clone()
    }

    /// The Layer and its dentry this answers through, as `ovl_path_real` finds
    /// the real path.
    pub(super) fn path_real(&self) -> OverlayPath {
        lock(&self.real).clone()
    }

    /// This file's data now stands at `real`, in the upper, as
    /// `ovl_dentry_set_upper_alias` records a copy up.
    pub(super) fn set_real(&self, real: OverlayPath) {
        *lock(&self.real) = real;
    }
}

/// Keep on `inode` what its lookup found, as `ovl_inode_init` takes it from
/// `ovl_lookup` under the directory's lock, so a name moved meanwhile cannot
/// have the entry stand for another.
pub(super) fn ovl_inode_init<T: Any + Send + Sync>(inode: &Inode, kept: T) {
    inode.private().get_or_init(|| Arc::new(kept));
}
