//! One mounted instance of a Filesystem, as Linux's `super_block` is.

use std::sync::Arc;

use super::dentry::Dentries;
use super::inode::{Inode, InodeOperations};
use crate::locking::RwSemaphore;

/// One Filesystem instance, with the directories of it a walk reaches, as a
/// Host App names it through `filesystem::Filesystem`. Every Mount of the same
/// instance shares them, as a bind mount shares its super block, so a directory
/// moved or removed beneath one Mount is moved or removed beneath every other.
pub struct SuperBlock {
    root: Arc<dyn InodeOperations>,
    dentries: Dentries,
    /// Held by a move between two directories, as Linux's
    /// `s_vfs_rename_mutex`, so no other such move changes which of them
    /// stands beneath the other.
    rename_mutex: Arc<RwSemaphore>,
}

impl SuperBlock {
    /// The instance `root` answers for.
    pub fn new(root: Arc<dyn InodeOperations>) -> Arc<Self> {
        let dentries = Dentries::new(root.dentry_ops());
        dentries.root().add(Some(Inode::root(root.clone())));
        Arc::new(Self {
            root,
            dentries,
            rename_mutex: Arc::default(),
        })
    }

    pub(crate) fn rename_mutex(&self) -> &Arc<RwSemaphore> {
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
