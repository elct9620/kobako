//! The directories of an Overlay opened as Filesystems of their own, each
//! holding its Layers' directories as overlayfs's `ovl_inode` holds its
//! upper and lower dentries, so a call through it asks those Layers
//! directly.
//!
//! A directory the upper does not hold yet is shared by every handle of
//! it, so a copy up through one is seen through all, as overlayfs sets the
//! upper dentry on the one inode. Such a directory never moves through the
//! Overlay, which refuses to move what a lower Layer holds, so it is kept
//! by its path; a removal or a replacement through the Overlay forgets it.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use super::super::path::is_beneath;
use super::path::OverlayPath;
use super::stack::Level;
use crate::hashtable::Hashtable;
use crate::vfs::RwSemaphore;

/// The lock a copy up of each path of the view holds, as overlayfs's
/// `ovl_inode` holds `lock` across `ovl_copy_up_start`, so two calls never
/// copy one file or directory up at once.
#[derive(Default)]
pub(super) struct InodeLocks(Mutex<Hashtable<String, RwSemaphore>>);

impl InodeLocks {
    /// The lock of `path`, kept while a call holds it.
    pub(super) fn get(&self, path: &str) -> Arc<RwSemaphore> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get_or_insert_with(path.to_string(), Arc::default)
    }
}

/// One directory of the view and the Layers' directories it stands for.
pub(super) struct OverlayInode {
    level: Mutex<Level>,
    /// Where it stood beneath the Overlay's root when it was opened, by
    /// which a change through it reaches its upper's directory.
    path: String,
}

impl OverlayInode {
    pub(super) fn new(level: Level, path: String) -> Arc<Self> {
        Arc::new(Self {
            level: Mutex::new(level),
            path,
        })
    }

    pub(super) fn level(&self) -> Level {
        self.lock().clone()
    }

    fn lock(&self) -> MutexGuard<'_, Level> {
        self.level.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(super) fn path(&self) -> &str {
        &self.path
    }
}

/// Every opened directory of one Overlay that the upper does not hold yet,
/// by its path.
#[derive(Default)]
pub(super) struct Inodes(Mutex<Hashtable<String, OverlayInode>>);

impl Inodes {
    fn lock(&self) -> MutexGuard<'_, Hashtable<String, OverlayInode>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The directory at `path`, standing for `level` as just found, as
    /// overlayfs's `ovl_get_inode` finds one: the one already shared there,
    /// brought up to date, where the upper is missing from `level`,
    /// or one of its own otherwise.
    pub(super) fn get_inode(&self, path: String, level: Level, shared: bool) -> Arc<OverlayInode> {
        if !shared {
            return OverlayInode::new(level, path);
        }
        let mut inodes = self.lock();
        if let Some(inode) = inodes.get(&path) {
            *inode.lock() = level;
            return inode;
        }
        inodes.get_or_insert_with(path.clone(), || OverlayInode::new(level, path))
    }

    /// Layer `into` now holds `dir` as the directory at `path`, made or
    /// found there by a copy up, as overlayfs's `ovl_inode_update` records
    /// the upper dentry, so every handle of it asks that Layer too.
    pub(super) fn inode_update(&self, path: &str, into: usize, dir: &OverlayPath) {
        if let Some(inode) = self.lock().get(&path.to_string()) {
            inode.lock()[into] = Some(dir.clone());
        }
    }

    /// The directory at `path` was removed or replaced through the
    /// Overlay, so one made there later is no handle's of it.
    pub(super) fn forget(&self, path: &str) {
        self.lock()
            .retain(|held| held != path && !is_beneath(held, path));
    }
}
