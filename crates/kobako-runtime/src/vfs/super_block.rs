//! One mounted instance of a Filesystem, as Linux's `super_block` is.

use std::sync::Arc;

use super::dentry::{Dentries, DentryOperations};
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
    /// Whether nothing beneath it changes, however it is mounted, as
    /// `SB_RDONLY` in `s_flags`.
    rdonly: bool,
}

impl SuperBlock {
    /// The instance `root` answers for, set up as its `fill_super` asks.
    pub fn new(root: Arc<dyn InodeOperations>) -> Arc<Self> {
        let mut sb = Self {
            root: root.clone(),
            dentries: Dentries::new(None),
            rename_mutex: Arc::default(),
            rdonly: false,
        };
        root.fill_super(&mut sb);
        sb.dentries.root().add(Some(Inode::root(root)));
        Arc::new(sb)
    }

    /// Have every dentry of this instance answer through `ops`, as a driver's
    /// `fill_super` calls `set_default_d_op` before any dentry is made.
    pub fn set_default_d_op(&mut self, ops: Arc<dyn DentryOperations>) {
        self.dentries = Dentries::new(Some(ops));
    }

    /// What every dentry of this instance answers through, as `s_d_op`.
    pub fn d_op(&self) -> Option<&Arc<dyn DentryOperations>> {
        self.dentries.ops()
    }

    /// Refuse every change beneath this instance, as a driver's `fill_super`
    /// sets `SB_RDONLY`.
    pub fn set_rdonly(&mut self) {
        self.rdonly = true;
    }

    /// Whether nothing beneath this instance changes, as `sb_rdonly`.
    pub fn rdonly(&self) -> bool {
        self.rdonly
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
