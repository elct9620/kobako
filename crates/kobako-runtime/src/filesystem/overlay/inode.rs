//! The entries of an Overlay's view as it keeps them on their inodes, as
//! overlayfs keeps an `ovl_inode` for each: a directory's Layers'
//! directories, the lock a copy up of the entry holds, and the copy a file
//! open to read reads once a copy up made one.
//!
//! The runtime records one inode for each dentry it reaches, so every handle
//! of an entry reaches the same state, and a removal or a replacement
//! through the runtime leaves the next lookup an inode, and so a state, of
//! its own.

use std::any::Any;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use super::super::{FileOperations, FilesystemError, OpenOptions};
use super::path::OverlayPath;
use super::stack::Level;
use crate::locking::RwSemaphore;
use crate::vfs::{Dentry, Inode};

/// A copy a file open to read reads once a copy up made it.
pub(super) type Copy = Arc<Mutex<Box<dyn FileOperations>>>;

/// One entry of the view.
pub(super) struct OverlayInode {
    /// Each Layer's directory a directory stands for; empty for a file.
    level: Mutex<Level>,
    /// The lock a copy up of the entry holds, as overlayfs's
    /// `ovl_inode_lock`, so two calls never copy one entry up at once.
    pub(super) lock: Arc<RwSemaphore>,
    /// How many files of a lower Layer are open to read here, so a copy up
    /// opens its copy only while one is.
    readers: AtomicUsize,
    copy: Mutex<Option<Copy>>,
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
            readers: AtomicUsize::new(0),
            copy: Mutex::new(None),
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

    /// The Layer's dentry this file answers through.
    pub(super) fn real(&self) -> Option<Arc<Dentry>> {
        lock(&self.real).clone()
    }

    /// The copy a copy up made, once one did for a file read here.
    pub(super) fn copy(&self) -> Option<Copy> {
        lock(&self.copy).clone()
    }

    /// Count a file of a lower Layer open to read here until the guard is
    /// dropped.
    pub(super) fn hold_reader(self: &Arc<Self>) -> ReaderGuard {
        self.readers.fetch_add(1, Ordering::SeqCst);
        ReaderGuard(self.clone())
    }

    /// This file was copied up as `name` into `dir`, the upper's
    /// directory; a file opened to read here reads the copy from now on,
    /// as overlayfs's open file reaches the upper inode.
    pub(super) fn copied_up(&self, dir: &OverlayPath, name: &str) -> Result<(), FilesystemError> {
        *lock(&self.real) = Some(dir.real(name)?);
        if self.readers.load(Ordering::SeqCst) == 0 {
            return Ok(());
        }
        let copy = dir.open(
            name,
            &OpenOptions {
                read: true,
                ..OpenOptions::default()
            },
        )?;
        *lock(&self.copy) = Some(Arc::new(Mutex::new(copy)));
        Ok(())
    }
}

/// One file of a lower Layer counted as open to read at an entry, until it
/// is dropped.
pub(super) struct ReaderGuard(Arc<OverlayInode>);

impl std::ops::Deref for ReaderGuard {
    type Target = OverlayInode;

    fn deref(&self) -> &OverlayInode {
        &self.0
    }
}

impl Drop for ReaderGuard {
    fn drop(&mut self) {
        self.0.readers.fetch_sub(1, Ordering::SeqCst);
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
pub(super) fn ovl_inode_kept(inode: &Inode) -> Option<Arc<OverlayInode>> {
    Some(downcast(inode.private().get()?.clone()))
}

fn downcast(kept: Arc<dyn Any + Send + Sync>) -> Arc<OverlayInode> {
    kept.downcast()
        .expect("an Overlay's inode keeps an Overlay's state")
}
