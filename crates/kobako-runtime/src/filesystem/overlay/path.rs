//! One Layer's directory as an Overlay holds it: the directory and its
//! dentry in the Layer's Filesystem, as overlayfs's `ovl_path` pairs a
//! layer with its dentry and looks a name up beneath it with `lookup_one`,
//! so the Layer
//! is asked about a name through a dentry the VFS made, and a directory
//! removed or moved through the Overlay is removed or moved in the
//! Layer's dentries too.

use std::sync::Arc;

use super::super::{DirEntry, File, FilesystemError, Inode, Metadata, OpenOptions, Rename};
use crate::vfs::{Dentry, Filesystem};

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

    /// Move `from` here to `to` in `target`, a directory of the same
    /// Layer.
    pub(super) fn rename(
        &self,
        from: &str,
        target: &Self,
        to: &str,
        rename: Rename,
    ) -> Result<(), FilesystemError> {
        self.dir.rename(
            &self.child(from),
            target.dir.as_ref(),
            &target.child(to),
            rename,
        )?;
        let dentries = self.filesystem.dentries();
        dentries.delete(&target.dentry, to);
        dentries.d_move(&self.dentry, from, &target.dentry, to);
        Ok(())
    }
}
