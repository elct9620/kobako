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
    DirEntry, FileOperations, FileType, Filesystem, FilesystemError, InodeOperations, Metadata,
    OpenOptions, Tmpfs,
};
use crate::locking::RwSemaphoreGuard;
use crate::vfs::{
    atomic_open, lookup_one, lookup_slow, start_renaming, vfs_link, vfs_mkdir, vfs_rename,
    vfs_rmdir, vfs_unlink, vfs_whiteout, Dentry, RenameFlag,
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

    /// Where this directory stands in its Layer.
    pub(super) fn dentry(&self) -> &Arc<Dentry> {
        &self.dentry
    }

    /// The dentry of `name` in this directory.
    fn child(&self, name: &str) -> Result<Arc<Dentry>, FilesystemError> {
        lookup_one(&self.filesystem, self.dir.as_ref(), &self.dentry, name)
    }

    /// Whether the dentry this Layer holds for `name`, if it holds one, still
    /// names what it recorded, as `ovl_revalidate_real` asks the real dentry.
    pub(super) fn revalidate_real(&self, name: &str) -> Result<bool, FilesystemError> {
        let held = self.filesystem.dentries().lookup(&self.dentry, name);
        match held.as_ref().and_then(|held| Some((held, held.ops()?))) {
            Some((held, ops)) => ops.revalidate(self.dir.as_ref(), held),
            None => Ok(true),
        }
    }

    /// The dentry of `name` as this directory stores it, what stands there
    /// recorded on it. The caller holds this directory's lock.
    fn lookup_real(&self, name: &str) -> Result<Arc<Dentry>, FilesystemError> {
        Ok(lookup_slow(&self.filesystem, self.dir.as_ref(), &self.dentry, name)?.1)
    }

    fn inode_lock_shared(&self) -> Result<RwSemaphoreGuard, FilesystemError> {
        self.dentry.inode_lock_shared()
    }

    fn inode_lock(&self) -> Result<RwSemaphoreGuard, FilesystemError> {
        self.dentry.inode_lock()
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

    pub(super) fn remove_opaque(&self) -> Result<(), FilesystemError> {
        let _held = self.inode_lock()?;
        self.upper()?.removexattr(OPAQUE_XATTR);
        Ok(())
    }

    pub(super) fn whiteout(&self, name: &str) -> Result<(), FilesystemError> {
        let _held = self.inode_lock()?;
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
        let rd = start_renaming(
            &self.filesystem,
            (self.dir.as_ref(), &self.dentry, from),
            (target.dir.as_ref(), &target.dentry, to),
            flag,
        )?;
        vfs_rename(
            &self.filesystem,
            (self.dir.as_ref(), &self.dentry, &rd.old_dentry),
            (target.dir.as_ref(), &target.dentry, &rd.new_dentry),
            flag,
            || self.upper()?.rename2(from, target.upper()?, to, flag),
        )
    }

    pub(super) fn tmpfile(&self) -> Result<TmpfsFile, FilesystemError> {
        self.upper()?.tmpfile()
    }

    pub(super) fn link(&self, file: &TmpfsFile, name: &str) -> Result<(), FilesystemError> {
        let _held = self.inode_lock()?;
        let child = self.child(name)?;
        vfs_link(self.dir.as_ref(), &self.dentry, &child, || {
            self.upper()?.link(file, name)
        })
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

    /// Open the file standing at `name`.
    pub(super) fn open(
        &self,
        name: &str,
        options: &OpenOptions,
    ) -> Result<Box<dyn FileOperations>, FilesystemError> {
        let _held = self.inode_lock_shared()?;
        self.lookup_real(name)?.open(options)
    }

    /// Make the file `name` and open it, holding the directory, as
    /// `O_CREAT` does.
    pub(super) fn atomic_open(
        &self,
        name: &str,
        options: &OpenOptions,
    ) -> Result<Box<dyn FileOperations>, FilesystemError> {
        let _held = self.inode_lock()?;
        atomic_open(
            self.dir.as_ref(),
            &self.dentry,
            &*self.child(name)?,
            options,
        )
    }

    /// The directory standing at `name`, a link there refused as a loop, as
    /// `ovl_lookup` follows no link in a Layer.
    pub(super) fn open_dir(&self, name: &str) -> Result<Self, FilesystemError> {
        let _held = self.inode_lock_shared()?;
        let dentry = self.lookup_real(name)?;
        let inode = dentry.inode().ok_or(FilesystemError::NotFound)?;
        match inode.kind() {
            FileType::Directory => Ok(Self {
                dir: inode.ops().clone(),
                dentry,
                filesystem: self.filesystem.clone(),
            }),
            FileType::Symlink => Err(FilesystemError::FilesystemLoop),
            _ => Err(FilesystemError::NotADirectory),
        }
    }

    pub(super) fn metadata(&self) -> Result<Metadata, FilesystemError> {
        self.dir.getattr(&self.dentry)
    }

    pub(super) fn symlink_metadata(&self, name: &str) -> Result<Metadata, FilesystemError> {
        let _held = self.inode_lock_shared()?;
        self.lookup_real(name)?.getattr()
    }

    /// `name` as this directory stores it.
    pub(super) fn lookup(&self, name: &str) -> Result<String, FilesystemError> {
        let _held = self.inode_lock_shared()?;
        Ok(lookup_slow(&self.filesystem, self.dir.as_ref(), &self.dentry, name)?.0)
    }

    /// This directory opened, kept open for whoever holds it, as overlayfs's
    /// `ovl_dir_open` opens the real directory.
    pub(super) fn open_real(&self) -> Option<Box<dyn FileOperations>> {
        let options = OpenOptions {
            read: true,
            directory: true,
            ..OpenOptions::default()
        };
        self.dir.open(&self.dentry, &options)?.ok()
    }

    pub(super) fn entries(&self) -> Result<Vec<DirEntry>, FilesystemError> {
        let _held = self.inode_lock_shared()?;
        let options = OpenOptions {
            read: true,
            directory: true,
            ..OpenOptions::default()
        };
        match self.dir.open(&self.dentry, &options) {
            Some(opened) => opened?.iterate_shared(&self.dentry),
            None => Err(FilesystemError::NotADirectory),
        }
    }

    pub(super) fn create_dir(&self, name: &str) -> Result<(), FilesystemError> {
        let _held = self.inode_lock()?;
        vfs_mkdir(self.dir.as_ref(), &self.dentry, &*self.child(name)?)
    }

    pub(super) fn remove_dir(&self, name: &str) -> Result<(), FilesystemError> {
        let _held = self.inode_lock()?;
        vfs_rmdir(
            &self.filesystem,
            self.dir.as_ref(),
            &self.dentry,
            &self.child(name)?,
        )
    }

    pub(super) fn remove_file(&self, name: &str) -> Result<(), FilesystemError> {
        let _held = self.inode_lock()?;
        vfs_unlink(
            &self.filesystem,
            self.dir.as_ref(),
            &self.dentry,
            &self.child(name)?,
        )
    }
}
