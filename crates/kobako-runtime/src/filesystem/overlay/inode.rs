//! The entries of an Overlay's view as it keeps them on their dentries,
//! as overlayfs keeps an `ovl_inode` for each: a directory's Layers'
//! directories, the lock a copy up of the entry holds, and the copy a file
//! open to read reads once a copy up made one.
//!
//! The runtime holds one dentry for each entry it reaches, so every handle
//! of an entry reaches the same state, and a removal or a replacement
//! through the runtime leaves the next lookup a dentry, and so a state, of
//! its own.

use std::any::Any;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use super::super::{File, FilesystemError, OpenOptions};
use super::path::OverlayPath;
use super::stack::Level;
use crate::vfs::{Dentry, RwSemaphore};

/// A copy a file open to read reads once a copy up made it.
pub(super) type Copy = Arc<Mutex<Box<dyn File>>>;

/// One entry of the view.
pub(super) struct OverlayInode {
    /// Each Layer's directory a directory stands for; empty for a file.
    level: Mutex<Level>,
    /// The lock a copy up of the entry holds, as overlayfs's
    /// `ovl_inode_lock`, so two calls never copy one entry up at once.
    pub(super) lock: RwSemaphore,
    /// How many files of a lower Layer are open to read here, so a copy up
    /// opens its copy only while one is.
    readers: AtomicUsize,
    copy: Mutex<Option<Copy>>,
}

fn lock<T>(held: &Mutex<T>) -> MutexGuard<'_, T> {
    held.lock().unwrap_or_else(PoisonError::into_inner)
}

impl OverlayInode {
    pub(super) fn level(&self) -> Level {
        lock(&self.level).clone()
    }

    /// Layer `into` now holds `dir` as this directory, made or found there
    /// by a copy up, as overlayfs's `ovl_inode_update` records the upper
    /// dentry, so every handle of it asks that Layer too.
    pub(super) fn inode_update(&self, into: usize, dir: &OverlayPath) {
        lock(&self.level)[into] = Some(dir.clone());
    }

    /// The copy a copy up made, once one did for a file read here.
    pub(super) fn copy(&self) -> Option<Copy> {
        lock(&self.copy).clone()
    }

    /// A file of a lower Layer was opened to read here.
    pub(super) fn opened_to_read(&self) {
        self.readers.fetch_add(1, Ordering::SeqCst);
    }

    /// A file of a lower Layer open to read here was let go.
    pub(super) fn closed_to_read(&self) {
        self.readers.fetch_sub(1, Ordering::SeqCst);
    }

    /// This file was copied up as `name` into `dir`, the upper's
    /// directory; a file opened to read here reads the copy from now on,
    /// as overlayfs's open file reaches the upper inode.
    pub(super) fn copied_up(&self, dir: &OverlayPath, name: &str) -> Result<(), FilesystemError> {
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

/// The state `dentry` keeps, made from the level `init` finds where it
/// keeps none yet, as overlayfs's `ovl_get_inode` finds or makes one.
pub(super) fn ovl_inode(
    dentry: &Dentry,
    init: impl FnOnce() -> Result<Level, FilesystemError>,
) -> Result<Arc<OverlayInode>, FilesystemError> {
    if let Some(kept) = dentry.d_fsdata.get() {
        return Ok(downcast(kept.clone()));
    }
    let made: Arc<dyn Any + Send + Sync> = Arc::new(OverlayInode {
        level: Mutex::new(init()?),
        lock: RwSemaphore::default(),
        readers: AtomicUsize::new(0),
        copy: Mutex::new(None),
    });
    Ok(downcast(dentry.d_fsdata.get_or_init(|| made).clone()))
}

fn downcast(kept: Arc<dyn Any + Send + Sync>) -> Arc<OverlayInode> {
    kept.downcast()
        .expect("an Overlay's dentry keeps an Overlay's state")
}
