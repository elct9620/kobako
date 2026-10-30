//! One Layer's directory as an Overlay holds it: the directory and its
//! dentry in the Layer's Filesystem, as overlayfs's `ovl_path` pairs a
//! layer with its dentry. Each call holds that directory's lock as the
//! VFS helpers overlayfs calls do: shared while a name is looked up, as
//! `lookup_one_unlocked`, and exclusive while one is made, removed, or
//! moved, as `start_creating`, `start_removing`, and `start_renaming`. A
//! name is found through the VFS's `lookup_one`, and an entry made,
//! removed, or moved through its `vfs_mkdir`, `vfs_create`, `vfs_rmdir`,
//! `vfs_unlink`, and `vfs_rename`, as overlayfs's are, so the VFS checks
//! what stands there and the Layer's dentries follow each change.

use std::any::Any;
use std::sync::Arc;

use super::super::tmpfs::TmpfsFile;
use super::super::{
    DirEntry, File, FilesystemError, InodeOperations, Metadata, OpenOptions, Tmpfs,
};
use crate::vfs::{
    lookup_one, start_renaming, vfs_create, vfs_link, vfs_mkdir, vfs_rename, vfs_rmdir, vfs_unlink,
    vfs_whiteout, Dentry, Filesystem, RenameFlag, RwSemaphoreGuard,
};

/// The attribute marking a directory opaque, and its value, as
/// overlayfs's `OVL_XATTR_OPAQUE`.
const OPAQUE_XATTR: &str = "trusted.overlay.opaque";
const OPAQUE: &[u8] = b"y";

#[derive(Clone)]
pub(super) struct OverlayPath {
    filesystem: Arc<Filesystem>,
    dir: Arc<dyn InodeOperations>,
    dentry: Arc<Dentry>,
}

impl OverlayPath {
    /// The root of the Layer `filesystem` stands for.
    pub(super) fn root(filesystem: &Arc<Filesystem>) -> Self {
        Self {
            dir: filesystem.root().clone(),
            dentry: filesystem.dentries().root(),
            filesystem: filesystem.clone(),
        }
    }

    /// The dentry of `name` in this directory.
    fn child(&self, name: &str) -> Result<Arc<Dentry>, FilesystemError> {
        lookup_one(&self.filesystem, self.dir.as_ref(), &self.dentry, name)
    }

    fn shared(&self) -> Result<RwSemaphoreGuard<'_>, FilesystemError> {
        self.dentry.rwsem.down_read(self.filesystem.blocking())
    }

    fn exclusive(&self) -> Result<RwSemaphoreGuard<'_>, FilesystemError> {
        self.dentry.rwsem.down_write(self.filesystem.blocking())
    }

    /// This directory as a Tmpfs, the one kind holding an Overlay's marks.
    fn tmpfs(&self) -> Option<&Tmpfs> {
        (self.dir.as_ref() as &dyn Any).downcast_ref::<Tmpfs>()
    }

    /// This directory of the upper, which is always a Tmpfs.
    fn upper(&self) -> Result<&Tmpfs, FilesystemError> {
        self.tmpfs().ok_or(FilesystemError::ReadOnlyFilesystem)
    }

    /// Whether a whiteout stands at `name`; a Layer that is no Tmpfs
    /// holds none, and one whose directory cannot be held is read as
    /// holding one, so nothing below shows through it.
    pub(super) fn is_whiteout(&self, name: &str) -> bool {
        self.tmpfs().is_some_and(|tmpfs| match self.shared() {
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
        let _held = self.exclusive()?;
        self.upper()?.setxattr(OPAQUE_XATTR, OPAQUE)
    }

    pub(super) fn remove_opaque(&self) -> Result<(), FilesystemError> {
        let _held = self.exclusive()?;
        self.upper()?.removexattr(OPAQUE_XATTR);
        Ok(())
    }

    pub(super) fn whiteout(&self, name: &str) -> Result<(), FilesystemError> {
        let _held = self.exclusive()?;
        let child = self.child(name)?;
        vfs_whiteout(self.dir.as_ref(), &self.dentry, &child, || {
            self.upper()?.whiteout(name)
        })
    }

    /// Move `from` here to `to` in `target` as `renameat2` does with
    /// `flag`.
    pub(super) fn rename2(
        &self,
        from: &str,
        target: &OverlayPath,
        to: &str,
        flag: Option<RenameFlag>,
    ) -> Result<(), FilesystemError> {
        let (old, new) = (self.child(from)?, target.child(to)?);
        let _held = start_renaming(
            &self.filesystem,
            (&self.dentry, &old),
            (&target.dentry, &new),
            flag,
        )?;
        vfs_rename(
            &self.filesystem,
            (self.dir.as_ref(), &self.dentry, &old),
            (target.dir.as_ref(), &target.dentry, &new),
            flag,
            || self.upper()?.rename2(from, target.upper()?, to, flag),
        )
    }

    pub(super) fn tmpfile(&self) -> Result<TmpfsFile, FilesystemError> {
        self.upper()?.tmpfile()
    }

    pub(super) fn link(&self, file: &TmpfsFile, name: &str) -> Result<(), FilesystemError> {
        let _held = self.exclusive()?;
        let child = self.child(name)?;
        vfs_link(self.dir.as_ref(), &self.dentry, &child, || {
            self.upper()?.link(file, name)
        })
    }

    pub(super) fn set_times(&self, metadata: &Metadata) -> Result<(), FilesystemError> {
        let _held = self.exclusive()?;
        self.upper()?
            .set_times(metadata.accessed, metadata.modified);
        Ok(())
    }

    /// Open the file standing at `name`.
    pub(super) fn open(
        &self,
        name: &str,
        options: &OpenOptions,
    ) -> Result<Box<dyn File>, FilesystemError> {
        let _held = self.shared()?;
        self.dir.open(&*self.child(name)?, options)
    }

    /// Make the file `name` and open it, holding the directory, as
    /// `O_CREAT` does.
    pub(super) fn create(
        &self,
        name: &str,
        options: &OpenOptions,
    ) -> Result<Box<dyn File>, FilesystemError> {
        let _held = self.exclusive()?;
        vfs_create(
            self.dir.as_ref(),
            &self.dentry,
            &*self.child(name)?,
            options,
        )
    }

    pub(super) fn open_dir(&self, name: &str) -> Result<Self, FilesystemError> {
        let _held = self.shared()?;
        let dentry = self.child(name)?;
        Ok(Self {
            dir: self.dir.open_dir(&dentry)?,
            dentry,
            filesystem: self.filesystem.clone(),
        })
    }

    pub(super) fn metadata(&self) -> Result<Metadata, FilesystemError> {
        self.dir.metadata(&self.dentry)
    }

    pub(super) fn symlink_metadata(&self, name: &str) -> Result<Metadata, FilesystemError> {
        let _held = self.shared()?;
        self.dir.symlink_metadata(&*self.child(name)?)
    }

    pub(super) fn lookup(&self, name: &str) -> Result<String, FilesystemError> {
        let _held = self.shared()?;
        self.dir.lookup(&*self.child(name)?)
    }

    pub(super) fn read_link(&self, name: &str) -> Result<String, FilesystemError> {
        let _held = self.shared()?;
        self.dir.read_link(&*self.child(name)?)
    }

    pub(super) fn entries(&self) -> Result<Vec<DirEntry>, FilesystemError> {
        let _held = self.shared()?;
        self.dir.entries(&self.dentry)
    }

    pub(super) fn create_dir(&self, name: &str) -> Result<(), FilesystemError> {
        let _held = self.exclusive()?;
        vfs_mkdir(self.dir.as_ref(), &self.dentry, &*self.child(name)?)
    }

    pub(super) fn remove_dir(&self, name: &str) -> Result<(), FilesystemError> {
        let _held = self.exclusive()?;
        vfs_rmdir(
            &self.filesystem,
            self.dir.as_ref(),
            &self.dentry,
            &self.child(name)?,
        )
    }

    pub(super) fn remove_file(&self, name: &str) -> Result<(), FilesystemError> {
        let _held = self.exclusive()?;
        vfs_unlink(
            &self.filesystem,
            self.dir.as_ref(),
            &self.dentry,
            &self.child(name)?,
        )
    }
}
