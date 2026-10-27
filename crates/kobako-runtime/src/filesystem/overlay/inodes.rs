//! The directories of an Overlay opened as Filesystems of their own, each
//! holding its Layers' directories as overlayfs's inode holds its upper and
//! lower dentries, so a call through it asks those Layers directly.
//!
//! A directory no writable Layer holds yet is shared by every handle of
//! it, so a copy up through one is seen through all, as overlayfs sets the
//! upper dentry on the one inode. Such a directory never moves through the
//! Overlay, which refuses to move what a lower Layer holds, so it is kept
//! by its path; a removal or a replacement through the Overlay forgets it.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

use super::super::path::is_beneath;
use super::lower::Lower;
use super::stack::Level;

/// One directory of the view and the Layers' directories it stands for.
pub(super) struct Inode {
    level: Mutex<Level>,
    /// Where it stood beneath the Overlay's root when it was opened, by
    /// which a change through it reaches its writable Layer's directory.
    path: String,
}

impl Inode {
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

/// Every opened directory of one Overlay that no writable Layer holds yet,
/// by its path.
#[derive(Default)]
pub(super) struct Inodes(Mutex<Shared>);

/// The shared directories, and how many the table may reach before those
/// no longer open are swept out: twice what the last sweep left, so each
/// opening pays a constant share of the sweeps.
#[derive(Default)]
struct Shared {
    entries: HashMap<String, Weak<Inode>>,
    sweep_at: usize,
}

impl std::ops::Deref for Shared {
    type Target = HashMap<String, Weak<Inode>>;

    fn deref(&self) -> &Self::Target {
        &self.entries
    }
}

impl std::ops::DerefMut for Shared {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.entries
    }
}

impl Inodes {
    fn lock(&self) -> MutexGuard<'_, Shared> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The directory at `path`, standing for `level` as just found: the
    /// one already shared there, brought up to date, where a writable Layer
    /// is missing from `level`, or one of its own otherwise.
    pub(super) fn at(&self, path: String, level: Level, shared: bool) -> Arc<Inode> {
        if !shared {
            return Inode::new(level, path);
        }
        let mut inodes = self.lock();
        if inodes.entries.len() >= inodes.sweep_at {
            inodes.entries.retain(|_, inode| inode.strong_count() > 0);
            inodes.sweep_at = (inodes.entries.len() * 2).max(16);
        }
        if let Some(inode) = inodes.get(&path).and_then(Weak::upgrade) {
            *inode.lock() = level;
            return inode;
        }
        let inode = Inode::new(level, path.clone());
        inodes.insert(path, Arc::downgrade(&inode));
        inode
    }

    /// Layer `into` now holds `dir` as the directory at `path`, made or
    /// found there by a copy up, so every handle of it asks that Layer too.
    pub(super) fn copied_up(&self, path: &str, into: usize, dir: &Lower) {
        if let Some(inode) = self.lock().get(path).and_then(Weak::upgrade) {
            inode.lock()[into] = Some(dir.clone());
        }
    }

    /// The directory at `path` was removed or replaced through the
    /// Overlay, so one made there later is no handle's of it.
    pub(super) fn forget(&self, path: &str) {
        self.lock()
            .retain(|held, _| held != path && !is_beneath(held, path));
    }
}
