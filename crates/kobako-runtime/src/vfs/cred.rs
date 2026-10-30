//! Who a call is made for, as Linux's `struct cred`, and the hooks a security
//! module answers, as `security_*` call each registered LSM. The VFS asks them
//! where Linux does and knows nothing of what they decide.

use std::sync::Arc;

use super::mount::{Denial, VfsMount};
use super::path::Path;
use super::Error;
use crate::filesystem::{Access, FileType, FilesystemError};

/// What a security module answers, as a hook in Linux's `lsm_hook_defs.h`. A
/// hook a module does not have allows the call, and a module can only refuse.
pub trait SecurityOperations: Send + Sync {
    /// Whether the path `path` names beneath `mnt` may be reached with
    /// `access`.
    fn inode_permission(
        &self,
        _mnt: &Arc<VfsMount>,
        _path: &dyn Fn() -> String,
        _access: Access,
    ) -> Result<(), Denial> {
        Ok(())
    }

    /// Whether a file of `kind` may be opened.
    fn file_open(&self, _kind: FileType) -> Result<(), FilesystemError> {
        Ok(())
    }

    /// Whether where a link points may be read.
    fn inode_readlink(&self) -> Result<(), FilesystemError> {
        Ok(())
    }

    /// Whether a walk beneath `mnt` may follow a link.
    fn inode_follow_link(&self, _mnt: &Arc<VfsMount>) -> Result<(), FilesystemError> {
        Ok(())
    }

    /// Whether an entry beneath `mnt`, a directory where `is_dir` says, may
    /// move.
    fn path_rename(&self, _mnt: &Arc<VfsMount>, _is_dir: bool) -> Result<(), FilesystemError> {
        Ok(())
    }

    /// Whether a call may start from `path`.
    fn path_walk(&self, _path: &Path) -> Result<(), Error> {
        Ok(())
    }
}

/// Who a call is made for: one per Invocation, holding the security modules
/// registered for it, as `cred->security` holds a Landlock domain.
#[derive(Default)]
pub struct Cred {
    security: Vec<Arc<dyn SecurityOperations>>,
}

impl Cred {
    /// One with `security` registered.
    pub fn new(security: Vec<Arc<dyn SecurityOperations>>) -> Arc<Self> {
        Arc::new(Self { security })
    }

    pub(crate) fn security_inode_permission(
        &self,
        mnt: &Arc<VfsMount>,
        path: &dyn Fn() -> String,
        access: Access,
    ) -> Result<(), Denial> {
        self.security
            .iter()
            .try_for_each(|module| module.inode_permission(mnt, path, access))
    }

    pub(crate) fn security_file_open(&self, kind: FileType) -> Result<(), FilesystemError> {
        self.security
            .iter()
            .try_for_each(|module| module.file_open(kind))
    }

    pub(crate) fn security_inode_readlink(&self) -> Result<(), FilesystemError> {
        self.security
            .iter()
            .try_for_each(|module| module.inode_readlink())
    }

    pub(crate) fn security_inode_follow_link(
        &self,
        mnt: &Arc<VfsMount>,
    ) -> Result<(), FilesystemError> {
        self.security
            .iter()
            .try_for_each(|module| module.inode_follow_link(mnt))
    }

    pub(crate) fn security_path_rename(
        &self,
        mnt: &Arc<VfsMount>,
        is_dir: bool,
    ) -> Result<(), FilesystemError> {
        self.security
            .iter()
            .try_for_each(|module| module.path_rename(mnt, is_dir))
    }

    pub(crate) fn security_path_walk(&self, path: &Path) -> Result<(), Error> {
        self.security
            .iter()
            .try_for_each(|module| module.path_walk(path))
    }
}
