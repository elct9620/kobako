//! Who a call is made for, as Linux's `struct cred`, and the hooks a security
//! module answers, as `security_*` call each registered LSM. The VFS asks them
//! where Linux does and knows nothing of what they decide.

use std::sync::Arc;

use super::dentry::Dentry;
use super::inode::Mask;
use super::mount::{Access, VfsMount};
use super::{FileType, FilesystemError};

/// A path a security hook is asked about: the mount it stands on, and its name
/// from that mount's root, built only for a module that reads it, as `struct path`
/// leaves `d_path` to the module that needs one.
#[derive(Clone, Copy)]
pub struct SecurityPath<'a> {
    pub mnt: &'a Arc<VfsMount>,
    pub name: &'a dyn Fn() -> String,
}

/// What a security module answers, as a hook in Linux's `lsm_hook_defs.h`. A hook
/// a module does not have allows the call, and a module can only refuse.
pub trait SecurityOperations: Send + Sync {
    /// Whether a call may do what `mask` asks of what `dentry` names, as
    /// `security_inode_permission` is asked once the entry itself allowed it.
    fn inode_permission(&self, _dentry: &Dentry, _mask: Mask) -> Result<(), FilesystemError> {
        Ok(())
    }

    /// Whether a file may be made at `path`, as `security_path_mknod`.
    fn path_mknod(&self, _path: SecurityPath<'_>) -> Result<(), FilesystemError> {
        Ok(())
    }

    /// Whether a directory may be made at `path`, as `security_path_mkdir`.
    fn path_mkdir(&self, _path: SecurityPath<'_>) -> Result<(), FilesystemError> {
        Ok(())
    }

    /// Whether the directory at `path` may go, as `security_path_rmdir`.
    fn path_rmdir(&self, _path: SecurityPath<'_>) -> Result<(), FilesystemError> {
        Ok(())
    }

    /// Whether the file at `path` may go, as `security_path_unlink`.
    fn path_unlink(&self, _path: SecurityPath<'_>) -> Result<(), FilesystemError> {
        Ok(())
    }

    /// Whether a link may be made at `path`, as `security_path_symlink`.
    fn path_symlink(&self, _path: SecurityPath<'_>) -> Result<(), FilesystemError> {
        Ok(())
    }

    /// Whether what `old` names may also go by `new`, as `security_path_link`.
    fn path_link(
        &self,
        _old: SecurityPath<'_>,
        _new: SecurityPath<'_>,
    ) -> Result<(), FilesystemError> {
        Ok(())
    }

    /// Whether what `old` names may move to `new`, the dentries of each given, as
    /// `security_path_rename` is handed both.
    fn path_rename(
        &self,
        _old: SecurityPath<'_>,
        _new: SecurityPath<'_>,
        _dentries: (&Arc<Dentry>, &Arc<Dentry>),
    ) -> Result<(), FilesystemError> {
        Ok(())
    }

    /// Whether a file of `kind` at `path` may be opened with `access`, as
    /// `security_file_open` is asked once `may_open` allowed it.
    fn file_open(
        &self,
        _path: SecurityPath<'_>,
        _kind: FileType,
        _access: Access,
    ) -> Result<(), FilesystemError> {
        Ok(())
    }

    /// Whether what `dentry` names may be truncated through an open file, as
    /// `security_file_truncate`.
    fn file_truncate(&self, _dentry: &Dentry) -> Result<(), FilesystemError> {
        Ok(())
    }

    /// Whether the entry at `path` may change, as `security_inode_setattr`.
    fn inode_setattr(&self, _path: SecurityPath<'_>) -> Result<(), FilesystemError> {
        Ok(())
    }

    /// Whether where a link points may be read.
    fn inode_readlink(&self) -> Result<(), FilesystemError> {
        Ok(())
    }

    /// Whether `bytes` more may be held for this caller, as
    /// `security_vm_enough_memory_mm` is asked before a Filesystem such as shmem
    /// holds memory on one's behalf.
    fn vm_enough_memory(&self, _bytes: usize) -> Result<(), FilesystemError> {
        Ok(())
    }

    /// `bytes` allowed earlier are held no more, as `vm_unacct_memory` gives them
    /// back.
    fn vm_unacct_memory(&self, _bytes: usize) {}
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

    /// Ask each module in turn, the first refusal answering, as `call_int_hook`.
    fn call_int_hook(
        &self,
        hook: impl Fn(&dyn SecurityOperations) -> Result<(), FilesystemError>,
    ) -> Result<(), FilesystemError> {
        self.security
            .iter()
            .try_for_each(|module| hook(module.as_ref()))
    }

    pub(crate) fn security_inode_permission(
        &self,
        dentry: &Dentry,
        mask: Mask,
    ) -> Result<(), FilesystemError> {
        self.call_int_hook(|module| module.inode_permission(dentry, mask))
    }

    pub(crate) fn security_path_mknod(
        &self,
        path: SecurityPath<'_>,
    ) -> Result<(), FilesystemError> {
        self.call_int_hook(|module| module.path_mknod(path))
    }

    pub(crate) fn security_path_mkdir(
        &self,
        path: SecurityPath<'_>,
    ) -> Result<(), FilesystemError> {
        self.call_int_hook(|module| module.path_mkdir(path))
    }

    pub(crate) fn security_path_rmdir(
        &self,
        path: SecurityPath<'_>,
    ) -> Result<(), FilesystemError> {
        self.call_int_hook(|module| module.path_rmdir(path))
    }

    pub(crate) fn security_path_unlink(
        &self,
        path: SecurityPath<'_>,
    ) -> Result<(), FilesystemError> {
        self.call_int_hook(|module| module.path_unlink(path))
    }

    pub(crate) fn security_path_symlink(
        &self,
        path: SecurityPath<'_>,
    ) -> Result<(), FilesystemError> {
        self.call_int_hook(|module| module.path_symlink(path))
    }

    pub(crate) fn security_path_link(
        &self,
        old: SecurityPath<'_>,
        new: SecurityPath<'_>,
    ) -> Result<(), FilesystemError> {
        self.call_int_hook(|module| module.path_link(old, new))
    }

    pub(crate) fn security_path_rename(
        &self,
        old: SecurityPath<'_>,
        new: SecurityPath<'_>,
        dentries: (&Arc<Dentry>, &Arc<Dentry>),
    ) -> Result<(), FilesystemError> {
        self.call_int_hook(|module| module.path_rename(old, new, dentries))
    }

    pub(crate) fn security_file_open(
        &self,
        path: SecurityPath<'_>,
        kind: FileType,
        access: Access,
    ) -> Result<(), FilesystemError> {
        self.call_int_hook(|module| module.file_open(path, kind, access))
    }

    pub(crate) fn security_file_truncate(&self, dentry: &Dentry) -> Result<(), FilesystemError> {
        self.call_int_hook(|module| module.file_truncate(dentry))
    }

    pub(crate) fn security_inode_setattr(
        &self,
        path: SecurityPath<'_>,
    ) -> Result<(), FilesystemError> {
        self.call_int_hook(|module| module.inode_setattr(path))
    }

    pub(crate) fn security_inode_readlink(&self) -> Result<(), FilesystemError> {
        self.call_int_hook(|module| module.inode_readlink())
    }

    pub(crate) fn security_vm_enough_memory(&self, bytes: usize) -> Result<(), FilesystemError> {
        self.call_int_hook(|module| module.vm_enough_memory(bytes))
    }

    pub(crate) fn vm_unacct_memory(&self, bytes: usize) {
        self.security
            .iter()
            .for_each(|module| module.vm_unacct_memory(bytes));
    }
}
