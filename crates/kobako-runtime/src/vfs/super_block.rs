//! One mounted instance of a Filesystem, as Linux's `super_block` is.

use std::sync::Arc;

use super::dentry::{Dentries, Dentry, DentryOperations};
use super::errno::FilesystemError;
use super::inode::Inode;
use crate::locking::RwSemaphore;

/// What makes an instance of a Filesystem, as Linux's `file_system_type`.
/// Every lock beneath it waits through the one `Blocking` its process
/// registers, so a Filesystem says nothing about waiting.
pub trait FileSystemType: Send + Sync + 'static {
    /// Set up the instance, asked once before any walk, as a driver's
    /// `fill_super` fills in its `super_block`. It makes the root through
    /// `d_make_root` after any `set_default_d_op`, so the root answers through
    /// it too. It also says what every dentry answers through, for a
    /// Filesystem changed outside the runtime, whether nothing beneath it
    /// changes, and whether a name goes once unused. An instance that cannot stand answers why, and
    /// none is made.
    fn fill_super(self: Arc<Self>, sb: &mut SuperBlock) -> Result<(), FilesystemError>;
}

/// One Filesystem instance, with the directories of it a walk reaches. Every
/// mount of the same instance shares them, as a bind mount shares its super
/// block, so a directory moved or removed beneath one is moved or removed
/// beneath every other.
pub struct SuperBlock {
    /// The root dentry `fill_super` made, as `s_root`.
    root: Option<Arc<Dentry>>,
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
        sb.root.as_ref().ok_or(FilesystemError::InvalidInput)?;
        Ok(Arc::new(sb))
    }

    /// Make a root dentry for `root` and set it as this instance's root, as a
    /// driver's `fill_super` sets `s_root` to what `d_make_root` makes.
    pub fn d_make_root(&mut self, root: Arc<Inode>) {
        self.root = Some(self.dentries.make_root(root));
    }

    /// This instance's root dentry, which `new` refuses an instance without.
    pub(crate) fn s_root(&self) -> Arc<Dentry> {
        self.root
            .clone()
            .expect("SuperBlock::new refuses an instance with no root")
    }

    /// Have every dentry of this instance made from now on answer through
    /// `ops`, as a driver's `fill_super` calls `set_default_d_op` before
    /// `d_make_root`. Without it the runtime trusts every name it holds, as
    /// nothing changes the Filesystem outside the runtime.
    pub fn set_default_d_op(&mut self, ops: Arc<dyn DentryOperations>) {
        self.dentries.set_ops(ops);
    }

    /// What every dentry of this instance answers through, as `s_d_op`.
    pub fn d_op(&self) -> Option<&Arc<dyn DentryOperations>> {
        self.dentries.ops()
    }

    /// Refuse every change beneath this instance, as a driver's `fill_super`
    /// sets `SB_RDONLY`. Without it the instance refuses no change, though a
    /// read-only mount of it still does.
    pub fn set_rdonly(&mut self) {
        self.rdonly = true;
    }

    /// Whether nothing beneath this instance changes, as `sb_rdonly`.
    pub fn rdonly(&self) -> bool {
        self.rdonly
    }

    /// Keep no dentry of this instance once nothing holds it, as a driver with
    /// nothing to gain from a cache sets `DCACHE_DONTCACHE`, as tmpfs does.
    /// Without it a name stays for the next walk, one naming nothing too.
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

    /// This instance's root dentry, once `fill_super` made it, as `s_root`.
    pub fn root(&self) -> Option<&Arc<Dentry>> {
        self.root.as_ref()
    }

    pub(crate) fn dentries(&self) -> &Dentries {
        &self.dentries
    }
}
