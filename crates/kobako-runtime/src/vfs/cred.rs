//! Who a call is made for, as Linux's `struct cred`, and the hooks a security
//! module answers, as `security_*` call each registered LSM. The VFS asks them
//! where Linux does and knows nothing of what they decide.

use std::sync::Arc;

use super::dentry::Dentry;
use super::mount::{Access, VfsMount};
use super::path::Path;
use super::{FileType, FilesystemError};

/// What a security module answers, as a hook in Linux's `lsm_hook_defs.h`,
/// beside one added at the start of a walk. A hook a module does not have allows
/// the call, and a module can only refuse.
pub trait SecurityOperations: Send + Sync {
    /// Whether the path `path` names beneath `mnt` may be reached with
    /// `access`, as AppArmor's `aa_path_perm` judges a whole path once.
    fn path_perm(
        &self,
        _mnt: &Arc<VfsMount>,
        _path: &dyn Fn() -> String,
        _access: Access,
    ) -> Result<(), FilesystemError> {
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

    /// Whether what `old` names beneath `mnt` may move to `new`, as
    /// `security_path_rename` is handed both dentries.
    fn path_rename(
        &self,
        _mnt: &Arc<VfsMount>,
        _old: &Arc<Dentry>,
        _new: &Arc<Dentry>,
    ) -> Result<(), FilesystemError> {
        Ok(())
    }

    /// Whether a call may start from `path`, a hook Linux does not have, as it
    /// asks nothing of the dentry a walk starts from.
    fn path_walk(&self, _path: &Path) -> Result<(), FilesystemError> {
        Ok(())
    }
}

/// Who a call is made for, holding the security modules registered for it, as
/// `cred->security` holds a Landlock domain.
#[derive(Default)]
pub struct Cred {
    security: Vec<Arc<dyn SecurityOperations>>,
}

impl Cred {
    /// One with `security` registered.
    pub fn new(security: Vec<Arc<dyn SecurityOperations>>) -> Arc<Self> {
        Arc::new(Self { security })
    }

    pub(crate) fn security_path_perm(
        &self,
        mnt: &Arc<VfsMount>,
        path: &dyn Fn() -> String,
        access: Access,
    ) -> Result<(), FilesystemError> {
        self.security
            .iter()
            .try_for_each(|module| module.path_perm(mnt, path, access))
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

    pub(crate) fn security_path_rename(
        &self,
        mnt: &Arc<VfsMount>,
        old: &Arc<Dentry>,
        new: &Arc<Dentry>,
    ) -> Result<(), FilesystemError> {
        self.security
            .iter()
            .try_for_each(|module| module.path_rename(mnt, old, new))
    }

    pub(crate) fn security_path_walk(&self, path: &Path) -> Result<(), FilesystemError> {
        self.security
            .iter()
            .try_for_each(|module| module.path_walk(path))
    }
}
