//! One mounted instance of a Filesystem, as Linux's `super_block` is.

use std::sync::Arc;

use super::dentry::Dentries;
use super::rwsem::{Blocking, Inline, RwSemaphore};
use crate::filesystem::InodeOperations;

/// One Filesystem instance, with the directories of it a guest reaches.
/// Every Mount of the same instance shares them, as a bind mount shares
/// its super block, so a directory moved or removed beneath one Mount is
/// moved or removed beneath every other.
pub struct Filesystem {
    root: Arc<dyn InodeOperations>,
    dentries: Dentries,
    /// What a call waiting for a directory's lock gives up meanwhile.
    blocking: Arc<dyn Blocking>,
    /// Held by a move between two directories, as Linux's
    /// `s_vfs_rename_mutex`, so no other such move changes which of them
    /// stands beneath the other.
    rename_mutex: RwSemaphore,
}

impl Filesystem {
    /// The instance `root` answers for.
    pub fn new(root: Arc<dyn InodeOperations>) -> Arc<Self> {
        Self::with_blocking(root, Arc::new(Inline))
    }

    /// The instance `root` answers for, each call waiting for a
    /// directory's lock through `blocking`.
    pub fn with_blocking(root: Arc<dyn InodeOperations>, blocking: Arc<dyn Blocking>) -> Arc<Self> {
        Arc::new(Self {
            root,
            dentries: Dentries::default(),
            blocking,
            rename_mutex: RwSemaphore::default(),
        })
    }

    pub(crate) fn blocking(&self) -> &dyn Blocking {
        self.blocking.as_ref()
    }

    pub(crate) fn rename_mutex(&self) -> &RwSemaphore {
        &self.rename_mutex
    }

    /// The Filesystem's root directory.
    pub fn root(&self) -> &Arc<dyn InodeOperations> {
        &self.root
    }

    pub(crate) fn dentries(&self) -> &Dentries {
        &self.dentries
    }
}
