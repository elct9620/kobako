//! One Layer's directory as an Overlay holds it: the directory and its
//! dentry in the Layer's Filesystem, as overlayfs's `ovl_path` pairs a
//! layer with its dentry and looks a name up beneath it with `lookup_one`,
//! so the Layer
//! is asked about a name through a dentry the VFS made, and a directory
//! removed or moved through the Overlay is removed or moved in the
//! Layer's dentries too.

use std::any::Any;
use std::sync::Arc;

use super::super::tmpfs::{RenameFlag, TmpfsFile};
use super::super::{DirEntry, File, FilesystemError, Inode, Metadata, OpenOptions, Tmpfs};
use crate::vfs::{Dentry, Filesystem};

/// The attribute marking a directory opaque, and its value, as
/// overlayfs's `OVL_XATTR_OPAQUE`.
const OPAQUE_XATTR: &str = "trusted.overlay.opaque";
const OPAQUE: &[u8] = b"y";

#[derive(Clone)]
pub(super) struct OverlayPath {
    filesystem: Arc<Filesystem>,
    dir: Arc<dyn Inode>,
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
    fn child(&self, name: &str) -> Arc<Dentry> {
        self.filesystem.dentries().child(&self.dentry, name)
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
    /// holds none.
    pub(super) fn is_whiteout(&self, name: &str) -> bool {
        self.tmpfs().is_some_and(|tmpfs| tmpfs.is_whiteout(name))
    }

    /// Whether this directory hides the Layers below it.
    pub(super) fn is_opaque(&self) -> bool {
        self.tmpfs()
            .and_then(|tmpfs| tmpfs.getxattr(OPAQUE_XATTR))
            .is_some_and(|value| value == OPAQUE)
    }

    pub(super) fn set_opaque(&self) -> Result<(), FilesystemError> {
        self.upper()?.setxattr(OPAQUE_XATTR, OPAQUE)
    }

    pub(super) fn remove_opaque(&self) -> Result<(), FilesystemError> {
        self.upper()?.removexattr(OPAQUE_XATTR);
        Ok(())
    }

    pub(super) fn whiteout(&self, name: &str) -> Result<(), FilesystemError> {
        self.upper()?.whiteout(name)
    }

    /// Move `from` here to `to` in `target` as `renameat2` does with
    /// `flag`. An exchanged directory is reached again by its new path.
    pub(super) fn rename2(
        &self,
        from: &str,
        target: &OverlayPath,
        to: &str,
        flag: Option<RenameFlag>,
    ) -> Result<(), FilesystemError> {
        self.upper()?.rename2(from, target.upper()?, to, flag)?;
        let dentries = self.filesystem.dentries();
        dentries.delete(&target.dentry, to);
        match flag {
            Some(RenameFlag::Exchange) => dentries.delete(&self.dentry, from),
            _ => dentries.d_move(&self.dentry, from, &target.dentry, to),
        }
        Ok(())
    }

    pub(super) fn tmpfile(&self) -> Result<TmpfsFile, FilesystemError> {
        self.upper()?.tmpfile()
    }

    pub(super) fn link(&self, file: &TmpfsFile, name: &str) -> Result<(), FilesystemError> {
        self.upper()?.link(file, name)
    }

    pub(super) fn set_times(&self, metadata: &Metadata) -> Result<(), FilesystemError> {
        self.upper()?
            .set_times(metadata.accessed, metadata.modified);
        Ok(())
    }

    pub(super) fn open(
        &self,
        name: &str,
        options: &OpenOptions,
    ) -> Result<Box<dyn File>, FilesystemError> {
        self.dir.open(&self.child(name), options)
    }

    pub(super) fn open_dir(&self, name: &str) -> Result<Self, FilesystemError> {
        let dentry = self.child(name);
        Ok(Self {
            dir: self.dir.open_dir(&dentry)?,
            dentry,
            filesystem: self.filesystem.clone(),
        })
    }

    pub(super) fn metadata(&self) -> Result<Metadata, FilesystemError> {
        self.dir.metadata()
    }

    pub(super) fn symlink_metadata(&self, name: &str) -> Result<Metadata, FilesystemError> {
        self.dir.symlink_metadata(&self.child(name))
    }

    pub(super) fn stored_name(&self, name: &str) -> Result<String, FilesystemError> {
        self.dir.stored_name(&self.child(name))
    }

    pub(super) fn read_link(&self, name: &str) -> Result<String, FilesystemError> {
        self.dir.read_link(&self.child(name))
    }

    pub(super) fn entries(&self) -> Result<Vec<DirEntry>, FilesystemError> {
        self.dir.entries()
    }

    pub(super) fn create_dir(&self, name: &str) -> Result<(), FilesystemError> {
        self.dir.create_dir(&self.child(name))
    }

    pub(super) fn remove_dir(&self, name: &str) -> Result<(), FilesystemError> {
        self.dir.remove_dir(&self.child(name))?;
        self.filesystem.dentries().delete(&self.dentry, name);
        Ok(())
    }

    pub(super) fn remove_file(&self, name: &str) -> Result<(), FilesystemError> {
        self.dir.remove_file(&self.child(name))
    }
}
