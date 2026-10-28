//! A test's way to ask a Filesystem about a path, as kernel code reaches a
//! file by name through `kern_path`: the VFS resolves the path to a
//! dentry, as it does for every call, and the Filesystem is asked about
//! that dentry, so a test sees the Filesystem's own answer.

use std::sync::Arc;

use super::namei::{filename_lookup, Nameidata};
use super::path::{Identity, Path};
use super::{Error, Filesystem};
use crate::filesystem::{
    Access, DirEntry, File, FilesystemError, Inode, Metadata, OpenOptions, Rename,
};
use crate::mount::Mount;
use crate::profile::Profile;

/// A directory of a Filesystem reached through the VFS.
#[derive(Clone)]
pub(crate) struct KernPath(Path);

/// The root of `filesystem`, mounted read-write.
pub(crate) fn kern_path(filesystem: Arc<dyn Inode>) -> KernPath {
    let mount = Mount::new("/data", Filesystem::new(filesystem), Access::ReadWrite)
        .expect("/data is a guest path");
    KernPath(Path::root(mount, Profile::Permissive))
}

fn to_filesystem_error(error: Error) -> FilesystemError {
    match error {
        Error::Filesystem(error) => error,
        Error::NotCapable => FilesystemError::NotPermitted,
        Error::Denied(denial) => panic!("a read-write Mount refused a call: {denial:?}"),
    }
}

impl KernPath {
    /// `path` resolved beneath this directory, a link at its end left as
    /// it stands.
    fn lookup(&self, path: &str) -> Result<Nameidata, FilesystemError> {
        filename_lookup(&self.0, path, false, false).map_err(to_filesystem_error)
    }

    pub(crate) fn open(
        &self,
        path: &str,
        options: &OpenOptions,
    ) -> Result<Box<dyn File>, FilesystemError> {
        let nd = self.lookup(path)?;
        nd.base().open(&nd.child(), options)
    }

    pub(crate) fn open_dir(&self, path: &str) -> Result<Self, FilesystemError> {
        let nd = self.lookup(path)?;
        let dentry = nd.child();
        let dir = nd.base().open_dir(&dentry)?;
        let identity = Identity::of(&dir.metadata()?);
        Ok(Self(self.0.with_directory(dir, dentry, identity)))
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
        let mut nd = self.lookup(path)?;
        nd.found()
    }

    pub(crate) fn stored_name(&self, path: &str) -> Result<String, FilesystemError> {
        let nd = self.lookup(path)?;
        nd.base().stored_name(&nd.child())
    }

    pub(crate) fn read_link(&self, path: &str) -> Result<String, FilesystemError> {
        let nd = self.lookup(path)?;
        nd.base().read_link(&nd.child())
    }

    pub(crate) fn entries(&self) -> Result<Vec<DirEntry>, FilesystemError> {
        self.0.entries().map_err(to_filesystem_error)
    }

    pub(crate) fn create_dir(&self, path: &str) -> Result<(), FilesystemError> {
        let nd = self.lookup(path)?;
        nd.base().create_dir(&nd.child())
    }

    pub(crate) fn remove_dir(&self, path: &str) -> Result<(), FilesystemError> {
        let nd = self.lookup(path)?;
        nd.base().remove_dir(&nd.child())
    }

    pub(crate) fn remove_file(&self, path: &str) -> Result<(), FilesystemError> {
        let nd = self.lookup(path)?;
        nd.base().remove_file(&nd.child())
    }

    /// Move `from` beneath this directory to `to` beneath `to_dir`, as
    /// `rename` allows.
    pub(crate) fn rename(
        &self,
        from: &str,
        to_dir: &Self,
        to: &str,
        rename: Rename,
    ) -> Result<(), FilesystemError> {
        let (source, target) = (self.lookup(from)?, to_dir.lookup(to)?);
        source.base().rename(
            &source.child(),
            target.base().as_ref(),
            &target.child(),
            rename,
        )
    }
}
