//! One mounted instance of a Filesystem, as Linux's `super_block` is.

use std::sync::Arc;

use super::dentry::{Dentries, DentryOperations};
use super::errno::FilesystemError;
use super::inode::{Inode, InodeOperations};
use super::stat::{FileType, Kstat};
use crate::locking::RwSemaphore;

/// What makes an instance of a Filesystem, as Linux's `file_system_type`.
pub trait FileSystemType: Send + Sync + 'static {
    /// Set up the instance, asked once before any walk, as a driver's
    /// `fill_super` fills in its `super_block`: its root through `d_make_root`,
    /// what every dentry answers through, for a Filesystem changed outside the
    /// runtime, and whether nothing beneath it changes. An instance that cannot
    /// stand answers why, and none is made.
    fn fill_super(self: Arc<Self>, sb: &mut SuperBlock) -> Result<(), FilesystemError>;
}

/// Make `root` the root of an instance with nothing else to set up, whose root
/// names no identity, as Linux's `simple_fill_super` fills one in.
pub fn simple_fill_super(
    sb: &mut SuperBlock,
    root: Arc<dyn InodeOperations>,
) -> Result<(), FilesystemError> {
    sb.d_make_root(Inode::new(&Kstat::new(FileType::Dir, 0), root));
    Ok(())
}

/// One Filesystem instance, with the directories of it a walk reaches. Every
/// mount of the same instance shares them, as a bind mount shares its super
/// block, so a directory moved or removed beneath one is moved or removed
/// beneath every other.
pub struct SuperBlock {
    /// What `fill_super` made its root, as `s_root`.
    root: Option<Arc<Inode>>,
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
    /// The instance `fs` makes, set up as its `fill_super` asks, or why it cannot
    /// stand. One whose `fill_super` made no root answers EINVAL.
    pub fn new(fs: Arc<dyn FileSystemType>) -> Result<Arc<Self>, FilesystemError> {
        let mut sb = Self {
            root: None,
            dentries: Dentries::new(None),
            rename_mutex: Arc::default(),
            rdonly: false,
            dontcache: false,
        };
        fs.fill_super(&mut sb)?;
        let root = sb.root.clone().ok_or(FilesystemError::InvalidInput)?;
        root.add_to(&sb.dentries.root());
        Ok(Arc::new(sb))
    }

    /// Make `root` the root of this instance, as a driver's `fill_super` sets
    /// `s_root` through `d_make_root`.
    pub fn d_make_root(&mut self, root: Arc<Inode>) {
        self.root = Some(root);
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

    /// What this instance's root stands for, once `fill_super` made it, as `s_root`.
    pub fn root(&self) -> Option<&Arc<Inode>> {
        self.root.as_ref()
    }

    pub(crate) fn dentries(&self) -> &Dentries {
        &self.dentries
    }
}
