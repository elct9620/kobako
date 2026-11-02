//! One mounted instance of a Filesystem, as Linux's `super_block` is.

use std::sync::Arc;

use super::dentry::{Dentries, DentryOperations};
use super::inode::{Inode, InodeOperations};
use super::stat::{FileType, Metadata};
use crate::locking::RwSemaphore;

/// What makes an instance of a Filesystem, as Linux's `file_system_type`, whose
/// value is also what answers for that instance's root.
pub trait FileSystemType: InodeOperations {
    /// Set up the instance, asked once before any walk, and answer what its root
    /// reports, as a driver's `fill_super` fills in its `super_block` and makes
    /// `s_root`: what every dentry answers through, for a Filesystem changed
    /// outside the runtime, and whether nothing beneath it changes. A Filesystem
    /// with nothing to set up reports a root with no identity.
    fn fill_super(&self, _sb: &mut SuperBlock) -> Metadata {
        Metadata::new(FileType::Directory, 0)
    }
}

/// One Filesystem instance, with the directories of it a walk reaches. Every
/// mount of the same instance shares them, as a bind mount shares its super
/// block, so a directory moved or removed beneath one is moved or removed
/// beneath every other.
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
    /// Whether no dentry stays once nothing holds it, as `DCACHE_DONTCACHE` in
    /// `s_d_flags`.
    dontcache: bool,
}

impl SuperBlock {
    /// The instance `fs` makes, set up as its `fill_super` asks.
    pub fn new(fs: Arc<dyn FileSystemType>) -> Arc<Self> {
        let root: Arc<dyn InodeOperations> = fs.clone();
        let mut sb = Self {
            root: root.clone(),
            dentries: Dentries::new(None),
            rename_mutex: Arc::default(),
            rdonly: false,
            dontcache: false,
        };
        let metadata = fs.fill_super(&mut sb);
        sb.dentries.root().add(Some(Inode::new(&metadata, root)));
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

    /// Keep no dentry of this instance once nothing holds it, as a driver with
    /// nothing to gain from a cache sets `DCACHE_DONTCACHE`, as tmpfs does.
    pub fn set_dontcache(&mut self) {
        self.dontcache = true;
    }

    /// Whether no dentry of this instance stays once nothing holds it.
    pub(crate) fn dontcache(&self) -> bool {
        self.dontcache
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
