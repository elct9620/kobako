//! Who a call is made for, as Linux's `struct cred`, and the hooks a security
//! module answers, as `security_*` call each registered LSM. The VFS asks them
//! where Linux does and knows nothing of what they decide.

use std::any::Any;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use super::dentry::Dentry;
use super::file::FMode;
use super::inode::Mask;
use super::mount::VfsMount;
use super::openat2::OpenHow;
use super::{FileType, FilesystemError};

/// What one security module keeps on an open file, as Linux's `f_security`
/// blob holds each module's part; `None` where it keeps nothing.
pub type FileSecurity = Option<Arc<dyn Any + Send + Sync>>;

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

    /// Whether a file of `kind` at `path` may be opened for `mode` as `how` asks,
    /// as `security_file_open` reads `f_mode` and `f_flags` once `may_open`
    /// allowed it, a file the open just made included, which `FMode::CREATED`
    /// marks. What the module keeps on the open file answers its later hooks, as
    /// Landlock keeps what it allowed in the file's security blob.
    fn file_open(
        &self,
        _path: SecurityPath<'_>,
        _kind: FileType,
        _mode: FMode,
        _how: &OpenHow,
    ) -> Result<FileSecurity, FilesystemError> {
        Ok(None)
    }

    /// Whether the open file may be truncated, judged from what this module kept
    /// on it as it opened, as Landlock's `hook_file_truncate` reads the file's
    /// blob.
    fn file_truncate(&self, _kept: &FileSecurity) -> Result<(), FilesystemError> {
        Ok(())
    }

    /// Whether times may be set on the entry at `path`. Linux has no path hook
    /// for times, so this is kobako's own, beside `path_truncate` and
    /// `path_chmod` in kind, letting a module judge the path a call names.
    fn path_utimes(&self, _path: SecurityPath<'_>) -> Result<(), FilesystemError> {
        Ok(())
    }

    /// Whether where the link `dentry` names points may be read, as
    /// `security_inode_readlink` is handed the link.
    fn inode_readlink(&self, _dentry: &Dentry) -> Result<(), FilesystemError> {
        Ok(())
    }
}

/// How many bytes Filesystems hold on one caller's behalf between calls, and the
/// most they may, as Linux's `__vm_enough_memory` weighs what `vm_committed_as`
/// counts against its limit, so a guest's descriptors cannot multiply a directory
/// past what it was allotted.
pub(crate) struct VmCommitted {
    limit: usize,
    used: AtomicUsize,
}

impl VmCommitted {
    pub(crate) fn new(limit: usize) -> Arc<Self> {
        Arc::new(Self {
            limit,
            used: AtomicUsize::new(0),
        })
    }
}

/// Who a call is made for, holding the security modules registered for it, as
/// `cred->security` holds a Landlock domain, and the memory held on its behalf,
/// which every Cred restricted from it shares, as a task's memory cgroup does.
pub struct Cred {
    security: Vec<Arc<dyn SecurityOperations>>,
    committed: Option<Arc<VmCommitted>>,
}

/// One no module is registered for, which this crate's tests make calls as.
#[cfg(test)]
impl Default for Cred {
    fn default() -> Self {
        Self {
            security: Vec::new(),
            committed: None,
        }
    }
}

impl Cred {
    /// One with `security` registered.
    pub(crate) fn new(security: Vec<Arc<dyn SecurityOperations>>) -> Arc<Self> {
        Self::within(security, None)
    }

    /// One with `security` registered and the memory held on its behalf counted
    /// in `committed`, without bound where none.
    pub(crate) fn within(
        security: Vec<Arc<dyn SecurityOperations>>,
        committed: Option<Arc<VmCommitted>>,
    ) -> Arc<Self> {
        Arc::new(Self {
            security,
            committed,
        })
    }

    /// This Cred with `more` modules registered after its own, sharing the
    /// memory held on its behalf, as `landlock_restrict_self` adds a domain.
    pub(crate) fn restricted(&self, more: Vec<Arc<dyn SecurityOperations>>) -> Arc<Self> {
        let mut security = self.security.clone();
        security.extend(more);
        Self::within(security, self.committed.clone())
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

    /// A file's room for what each module keeps on it, empty until an open
    /// fills it, as `security_file_alloc` gives every file one blob per module.
    pub(crate) fn security_file_alloc(&self) -> Vec<FileSecurity> {
        vec![None; self.security.len()]
    }

    /// Ask each module about the open, keeping what each keeps on the file in
    /// the order the modules stand, as `security_file_open` fills `f_security`.
    pub(crate) fn security_file_open(
        &self,
        path: SecurityPath<'_>,
        kind: FileType,
        mode: FMode,
        how: &OpenHow,
    ) -> Result<Vec<FileSecurity>, FilesystemError> {
        self.security
            .iter()
            .map(|module| module.file_open(path, kind, mode, how))
            .collect()
    }

    /// Ask each module whether the file may be truncated, handing each what it
    /// kept on the file as it opened.
    pub(crate) fn security_file_truncate(
        &self,
        kept: &[FileSecurity],
    ) -> Result<(), FilesystemError> {
        self.security
            .iter()
            .zip(kept)
            .try_for_each(|(module, kept)| module.file_truncate(kept))
    }

    pub(crate) fn security_path_utimes(
        &self,
        path: SecurityPath<'_>,
    ) -> Result<(), FilesystemError> {
        self.call_int_hook(|module| module.path_utimes(path))
    }

    pub(crate) fn security_inode_readlink(&self, dentry: &Dentry) -> Result<(), FilesystemError> {
        self.call_int_hook(|module| module.inode_readlink(dentry))
    }

    /// Count `bytes` more held on this caller's behalf, refusing them past its
    /// limit, as `security_vm_enough_memory_mm` ends in `__vm_enough_memory`.
    pub(crate) fn security_vm_enough_memory(&self, bytes: usize) -> Result<(), FilesystemError> {
        let Some(committed) = &self.committed else {
            return Ok(());
        };
        committed
            .used
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |used| {
                used.checked_add(bytes)
                    .filter(|&total| total <= committed.limit)
            })
            .map(|_| ())
            .map_err(|_| FilesystemError::OutOfMemory)
    }

    /// `bytes` counted earlier are held no more, as `vm_unacct_memory` gives
    /// them back.
    pub(crate) fn vm_unacct_memory(&self, bytes: usize) {
        if let Some(committed) = &self.committed {
            committed.used.fetch_sub(bytes, Ordering::Relaxed);
        }
    }
}
