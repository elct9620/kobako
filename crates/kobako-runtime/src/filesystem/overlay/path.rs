//! One Layer's directory as an Overlay holds it: the directory and its
//! dentry in the Layer's Filesystem, as overlayfs's `ovl_path` pairs a
//! layer with its dentry. Each call holds that directory's lock as the
//! VFS helpers overlayfs calls do: shared while a name is looked up, as
//! `lookup_one_unlocked`, and exclusive while one is made, removed, or
//! moved, as `start_creating`, `start_removing`, and `start_renaming`. A
//! name is found through the VFS's `lookup_one`, and an entry made,
//! removed, or moved through its `vfs_mkdir`, `atomic_open`, `vfs_rmdir`,
//! `vfs_unlink`, and `vfs_rename`, as overlayfs's are, so the VFS checks
//! what stands there and the Layer's dentries follow each change.

use std::any::Any;
use std::sync::Arc;

use super::super::tmpfs::TmpfsFile;
use super::super::{
    DirEntry, FileOperations, FileType, Filesystem, FilesystemError, Metadata, OpenOptions, Tmpfs,
};
use crate::locking::RwSemaphoreGuard;
use crate::vfs::{
    atomic_open, lookup_one, lookup_slow, may_create_dentry, start_renaming, vfs_mkdir, vfs_rename,
    vfs_rmdir, vfs_unlink, Dentry, RenameFlag,
};

/// The attribute marking a directory opaque, and its value, as
/// overlayfs's `OVL_XATTR_OPAQUE`.
const OPAQUE_XATTR: &str = "trusted.overlay.opaque";
const OPAQUE: &[u8] = b"y";

#[derive(Clone)]
pub(super) struct OverlayPath {
    filesystem: Arc<Filesystem>,
    dentry: Arc<Dentry>,
}

impl OverlayPath {
    /// The root of the Layer `filesystem` stands for.
    pub(super) fn root(filesystem: &Arc<Filesystem>) -> Self {
        Self {
            dentry: filesystem.dentries().root(),
            filesystem: filesystem.clone(),
        }
    }

    /// This Layer's directory standing at `dentry`.
    pub(super) fn with_dentry(&self, dentry: Arc<Dentry>) -> Self {
        Self {
            dentry,
            filesystem: self.filesystem.clone(),
        }
    }

    /// Where this directory stands in its Layer.
    pub(super) fn dentry(&self) -> &Arc<Dentry> {
        &self.dentry
    }

    /// The dentry of `name` in this directory.
    fn child(&self, name: &str) -> Result<Arc<Dentry>, FilesystemError> {
        lookup_one(&self.filesystem, &self.dentry, name)
    }

    /// Whether the dentry this Layer holds for `name`, if it holds one, still
    /// names what it recorded, as `ovl_revalidate_real` asks the real dentry.
    pub(super) fn revalidate_real(&self, name: &str) -> Result<bool, FilesystemError> {
        let held = self.filesystem.dentries().lookup(&self.dentry, name);
        match held.as_ref().and_then(|held| Some((held, held.ops()?))) {
            Some((held, ops)) => ops.revalidate(self.dentry.inode_ops()?.as_ref(), held),
            None => Ok(true),
        }
    }

    /// Whether what this Layer holds at `name`, if it holds a dentry there, is
    /// `real`, as `ovl_matches_upper` compares what stands in the upper.
    pub(super) fn matches_real(&self, name: &str, real: &Arc<Dentry>) -> bool {
        let held = self.filesystem.dentries().lookup(&self.dentry, name);
        held.is_none_or(|held| Arc::ptr_eq(&held, real))
    }

    /// The dentry of `name` as this directory stores it, what stands there
    /// recorded on it. The caller holds this directory's lock.
    fn lookup_real(&self, name: &str) -> Result<Arc<Dentry>, FilesystemError> {
        Ok(lookup_slow(&self.filesystem, &self.dentry, name)?.1)
    }

    fn inode_lock_shared(&self) -> Result<RwSemaphoreGuard, FilesystemError> {
        self.dentry.inode_lock_shared()
    }

    fn inode_lock(&self) -> Result<RwSemaphoreGuard, FilesystemError> {
        self.dentry.inode_lock()
    }

    /// This directory as a Tmpfs, the one kind holding an Overlay's marks.
    fn tmpfs(&self) -> Option<Arc<Tmpfs>> {
        let ops: Arc<dyn Any + Send + Sync> = self.dentry.inode_ops().ok()?;
        ops.downcast().ok()
    }

    /// This directory of the upper, which is always a Tmpfs.
    fn upper(&self) -> Result<Arc<Tmpfs>, FilesystemError> {
        self.tmpfs().ok_or(FilesystemError::ReadOnlyFilesystem)
    }

    /// Whether a whiteout stands at `name`; a Layer that is no Tmpfs
    /// holds none, and one whose directory cannot be held is read as
    /// holding one, so nothing below shows through it.
    pub(super) fn is_whiteout(&self, name: &str) -> bool {
        self.tmpfs()
            .is_some_and(|tmpfs| match self.inode_lock_shared() {
                Ok(_held) => tmpfs.is_whiteout(name),
                Err(_) => true,
            })
    }

    /// Whether this directory hides the Layers below it.
    pub(super) fn is_opaque(&self) -> bool {
        self.tmpfs()
            .and_then(|tmpfs| tmpfs.getxattr(OPAQUE_XATTR))
            .is_some_and(|value| value == OPAQUE)
    }

    pub(super) fn set_opaque(&self) -> Result<(), FilesystemError> {
        let _held = self.inode_lock()?;
        self.upper()?.setxattr(OPAQUE_XATTR, OPAQUE)
    }

    pub(super) fn do_whiteout(&self, name: &str) -> Result<(), FilesystemError> {
        let _held = self.inode_lock()?;
        may_create_dentry(&self.dentry, &*self.child(name)?)?;
        self.upper()?.whiteout(name)
    }

    /// Move `from` here to `to` in `target` as `renameat2` does with
    /// `flag`.
    pub(super) fn do_rename(
        &self,
        from: &str,
        target: &OverlayPath,
        to: &str,
        flag: Option<RenameFlag>,
    ) -> Result<(), FilesystemError> {
        let rd = start_renaming(
            &self.filesystem,
            (&self.dentry, from),
            (&target.dentry, to),
            flag,
        )?;
        vfs_rename(&self.filesystem, &rd)
    }

    pub(super) fn do_tmpfile(&self) -> Result<TmpfsFile, FilesystemError> {
        self.upper()?.tmpfile()
    }

    pub(super) fn do_link(&self, file: &TmpfsFile, name: &str) -> Result<(), FilesystemError> {
        let _held = self.inode_lock()?;
        may_create_dentry(&self.dentry, &*self.child(name)?)?;
        self.upper()?.link(file, name)
    }

    pub(super) fn set_times(&self, metadata: &Metadata) -> Result<(), FilesystemError> {
        let _held = self.inode_lock()?;
        self.upper()?
            .set_times(metadata.accessed, metadata.modified);
        Ok(())
    }

    /// The dentry of the entry standing at `name`.
    pub(super) fn real(&self, name: &str) -> Result<Arc<Dentry>, FilesystemError> {
        let _held = self.inode_lock_shared()?;
        self.lookup_real(name)
    }

    /// Make the file `name` and open it, holding the directory, as
    /// `O_CREAT` does.
    pub(super) fn atomic_open(
        &self,
        name: &str,
        options: &OpenOptions,
    ) -> Result<Box<dyn FileOperations>, FilesystemError> {
        let _held = self.inode_lock()?;
        atomic_open(&self.dentry, &*self.child(name)?, options)
    }

    /// The directory standing at `name`, a link there refused as a loop, as
    /// `ovl_lookup` follows no link in a Layer.
    pub(super) fn open_dir(&self, name: &str) -> Result<Self, FilesystemError> {
        let _held = self.inode_lock_shared()?;
        let dentry = self.lookup_real(name)?;
        let inode = dentry.inode().ok_or(FilesystemError::NotFound)?;
        match inode.kind() {
            FileType::Directory => Ok(Self {
                dentry,
                filesystem: self.filesystem.clone(),
            }),
            FileType::Symlink => Err(FilesystemError::FilesystemLoop),
            _ => Err(FilesystemError::NotADirectory),
        }
    }

    pub(super) fn metadata(&self) -> Result<Metadata, FilesystemError> {
        self.dentry.getattr()
    }

    pub(super) fn symlink_metadata(&self, name: &str) -> Result<Metadata, FilesystemError> {
        let _held = self.inode_lock_shared()?;
        self.lookup_real(name)?.getattr()
    }

    /// `name` as this directory stores it.
    pub(super) fn lookup(&self, name: &str) -> Result<String, FilesystemError> {
        let _held = self.inode_lock_shared()?;
        Ok(lookup_slow(&self.filesystem, &self.dentry, name)?.0)
    }

    /// This directory opened, kept open for whoever holds it, as overlayfs's
    /// `ovl_dir_open` opens the real directory.
    pub(super) fn open_real(&self) -> Result<Box<dyn FileOperations>, FilesystemError> {
        let options = OpenOptions {
            read: true,
            ..OpenOptions::default()
        };
        self.dentry.inode_ops()?.open(&self.dentry, &options)
    }

    pub(super) fn entries(&self) -> Result<Vec<DirEntry>, FilesystemError> {
        let _held = self.inode_lock_shared()?;
        let options = OpenOptions {
            read: true,
            ..OpenOptions::default()
        };
        self.dentry
            .inode_ops()?
            .open(&self.dentry, &options)?
            .iterate_shared(&self.dentry)
    }

    pub(super) fn do_mkdir(&self, name: &str) -> Result<(), FilesystemError> {
        let _held = self.inode_lock()?;
        vfs_mkdir(&self.dentry, &*self.child(name)?)
    }

    pub(super) fn do_rmdir(&self, name: &str) -> Result<(), FilesystemError> {
        let _held = self.inode_lock()?;
        vfs_rmdir(&self.filesystem, &self.dentry, &self.child(name)?)
    }

    pub(super) fn do_unlink(&self, name: &str) -> Result<(), FilesystemError> {
        let _held = self.inode_lock()?;
        vfs_unlink(&self.filesystem, &self.dentry, &self.child(name)?)
    }
}
