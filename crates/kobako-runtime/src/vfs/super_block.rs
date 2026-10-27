//! One mounted instance of a Filesystem, as Linux's `super_block` is.

use std::sync::Arc;

use super::dentry::Dentries;
use crate::filesystem::Inode;

/// One Filesystem instance, with the directories of it a guest reaches.
/// Every Mount of the same SuperBlock shares them, as a bind mount shares
/// its super block, so a directory moved or removed beneath one Mount is
/// moved or removed beneath every other.
pub struct SuperBlock {
    root: Arc<dyn Inode>,
    dentries: Dentries,
}

impl SuperBlock {
    /// The instance `root` answers for.
    pub fn new(root: Arc<dyn Inode>) -> Arc<Self> {
        Arc::new(Self {
            root,
            dentries: Dentries::default(),
        })
    }

    /// The Filesystem's root directory.
    pub fn root(&self) -> &Arc<dyn Inode> {
        &self.root
    }

    pub(crate) fn dentries(&self) -> &Dentries {
        &self.dentries
    }
}
