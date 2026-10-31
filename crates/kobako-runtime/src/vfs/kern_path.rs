//! A test's way to ask a Filesystem about a path, as kernel code reaches a
//! file by name through `kern_path`: the VFS resolves the path to a
//! dentry, as it does for every call, and the Filesystem is asked about
//! that dentry, a change going through the VFS's own `vfs_*` helpers as
//! kernel code's does, so a test sees what the Filesystem answers beneath
//! the VFS. It takes no directory's lock, so a test holding one can
//! still reach the Filesystem through it.

use std::sync::Arc;

use super::namei::{
    atomic_open, filename_lookup, vfs_mkdir, vfs_rename, vfs_rmdir, vfs_unlink, Nameidata,
    RenameData,
};
use super::path::Path;
use super::{
    DirEntry, Error, File, FileOperations, FileType, FilesystemError, Iattr, InodeOperations,
    Metadata, OpenOptions, SuperBlock,
};
use crate::mount::{Access, Mount};

/// A directory of a Filesystem reached through the VFS.
#[derive(Clone)]
pub(crate) struct KernPath(Path);

/// The root of `filesystem`, mounted read-write.
pub(crate) fn kern_path(filesystem: Arc<dyn InodeOperations>) -> KernPath {
    let mount = Mount::new("/data", SuperBlock::new(filesystem), Access::ReadWrite)
        .expect("/data is a guest path");
    KernPath(Path::root(mount.mnt()))
}

fn to_filesystem_error(error: Error) -> FilesystemError {
    match error {
        Error::Filesystem(error) => error,
        Error::Escape => FilesystemError::NotPermitted,
        Error::Denied(denial) => panic!("a read-write Mount refused a call: {denial:?}"),
    }
}

impl KernPath {
    /// `path` resolved beneath this directory, a link at its end left as
    /// it stands.
    fn resolve(&self, path: &str) -> Result<Nameidata, FilesystemError> {
        filename_lookup(&Arc::default(), &self.0, path, false).map_err(to_filesystem_error)
    }

    pub(crate) fn open(
        &self,
        path: &str,
        options: &OpenOptions,
    ) -> Result<Box<dyn FileOperations>, FilesystemError> {
        let child = self.resolve(path)?.child()?;
        let file = child.open(options)?;
        if options.truncate {
            child
                .inode()
                .ok_or(FilesystemError::NotFound)?
                .ops()
                .setattr(&child, &Iattr::open_truncate())?;
        }
        Ok(file)
    }

    /// Open the file at `path`, made first where nothing stands.
    pub(crate) fn open_or_create(
        &self,
        path: &str,
        options: &OpenOptions,
    ) -> Result<Box<dyn FileOperations>, FilesystemError> {
        match self.symlink_metadata(path) {
            Err(FilesystemError::NotFound) => self.create(path, options),
            _ => self.open(path, options),
        }
    }

    /// Make the file at `path` and open it, as `->atomic_open` does.
    pub(crate) fn create(
        &self,
        path: &str,
        options: &OpenOptions,
    ) -> Result<Box<dyn FileOperations>, FilesystemError> {
        let nd = self.resolve(path)?;
        let child = nd.child()?;
        atomic_open(nd.dentry(), &child, options)
    }

    /// Truncate or extend the file at `path`, as `->setattr` is asked.
    pub(crate) fn truncate(&self, path: &str, len: u64) -> Result<(), FilesystemError> {
        let nd = self.resolve(path)?;
        let child = nd.child()?;
        child
            .inode()
            .ok_or(FilesystemError::NotFound)?
            .ops()
            .setattr(&child, &Iattr::size(len))
    }

    pub(crate) fn open_dir(&self, path: &str) -> Result<Self, FilesystemError> {
        let nd = self.resolve(path)?;
        let dentry = nd.child()?;
        let inode = dentry.inode().ok_or(FilesystemError::NotFound)?;
        if inode.kind() == FileType::Symlink {
            return Err(FilesystemError::FilesystemLoop);
        }
        dentry.can_lookup(inode.ops())?;
        Ok(Self(self.0.with_dentry(dentry)))
    }

    /// What the directory reports of itself.
    pub(crate) fn metadata(&self) -> Result<Metadata, FilesystemError> {
        self.0.metadata().map_err(to_filesystem_error)
    }

    /// What `path` names, a link reported as itself; the directory itself
    /// for the empty path.
    pub(crate) fn symlink_metadata(&self, path: &str) -> Result<Metadata, FilesystemError> {
        if path.is_empty() {
            return self.metadata();
        }
        let mut nd = self.resolve(path)?;
        nd.getattr()
    }

    pub(crate) fn lookup(&self, path: &str) -> Result<String, FilesystemError> {
        Ok(self.resolve(path)?.name)
    }

    #[cfg(unix)]
    pub(crate) fn read_link(&self, path: &str) -> Result<String, FilesystemError> {
        let nd = self.resolve(path)?;
        nd.child()?.get_link()
    }

    pub(crate) fn entries(&self) -> Result<Vec<DirEntry>, FilesystemError> {
        File::directory(self.0.clone(), Arc::default())?.iterate_shared()
    }

    pub(crate) fn create_dir(&self, path: &str) -> Result<(), FilesystemError> {
        let nd = self.resolve(path)?;
        let child = nd.child()?;
        vfs_mkdir(nd.dentry(), &child)
    }

    pub(crate) fn remove_dir(&self, path: &str) -> Result<(), FilesystemError> {
        let nd = self.resolve(path)?;
        let child = nd.child()?;
        vfs_rmdir(nd.mnt.sb(), nd.dentry(), &child)
    }

    pub(crate) fn remove_file(&self, path: &str) -> Result<(), FilesystemError> {
        let nd = self.resolve(path)?;
        let child = nd.child()?;
        vfs_unlink(nd.mnt.sb(), nd.dentry(), &child)
    }

    /// Move `from` beneath this directory to `to` beneath `to_dir`,
    /// refused where the source stands above the target's directory, as
    /// `start_renaming` refuses it.
    pub(crate) fn rename(
        &self,
        from: &str,
        to_dir: &Self,
        to: &str,
    ) -> Result<(), FilesystemError> {
        let (source, target) = (self.resolve(from)?, to_dir.resolve(to)?);
        let (old, new) = (source.child()?, target.child()?);
        if target.dentry().is_subdir(&old) {
            return Err(FilesystemError::InvalidInput);
        }
        if source.dentry().is_subdir(&new) {
            return Err(FilesystemError::DirectoryNotEmpty);
        }
        vfs_rename(
            source.mnt.sb(),
            &RenameData {
                _held: Vec::new(),
                old_parent: source.dentry().clone(),
                old_dentry: old,
                new_parent: target.dentry().clone(),
                new_dentry: new,
                flag: None,
            },
        )
    }
}
