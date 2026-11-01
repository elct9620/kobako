//! Who a call is made for, as Linux's `struct cred`, and the hooks a security
//! module answers, as `security_*` call each registered LSM. The VFS asks them
//! where Linux does and knows nothing of what they decide.

use std::sync::atomic::{AtomicUsize, Ordering};
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
    rdirs: Arc<RdirAccount>,
}

/// How many bytes of listings the files of one caller hold between calls, as
/// `cred->user` counts a user's pipe buffers, so a guest's descriptors cannot
/// multiply a listing past what it was allotted.
struct RdirAccount {
    limit: usize,
    used: AtomicUsize,
}

impl Default for RdirAccount {
    fn default() -> Self {
        Self {
            limit: usize::MAX,
            used: AtomicUsize::new(0),
        }
    }
}

/// Bytes a file's listing counts for as long as it holds them.
pub(super) struct RdirCharge {
    account: Arc<RdirAccount>,
    bytes: usize,
}

impl Drop for RdirCharge {
    fn drop(&mut self) {
        self.account.used.fetch_sub(self.bytes, Ordering::Relaxed);
    }
}

impl Cred {
    /// One with `security` registered.
    pub fn new(security: Vec<Arc<dyn SecurityOperations>>) -> Arc<Self> {
        Arc::new(Self {
            security,
            rdirs: Arc::default(),
        })
    }

    /// This caller, whose files hold at most `limit` bytes of listings between
    /// calls, or without bound for `None`.
    pub fn with_rdir_limit(&self, limit: Option<usize>) -> Arc<Self> {
        Arc::new(Self {
            security: self.security.clone(),
            rdirs: Arc::new(RdirAccount {
                limit: limit.unwrap_or(usize::MAX),
                used: AtomicUsize::new(0),
            }),
        })
    }

    /// Count `bytes` of a listing a file goes on holding, refused as out of memory
    /// past the limit, as `alloc_pipe_info` accounts each pipe to `cred->user`.
    pub(super) fn account_rdir(&self, bytes: usize) -> Result<RdirCharge, FilesystemError> {
        let account = &self.rdirs;
        account
            .used
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |used| {
                used.checked_add(bytes)
                    .filter(|&total| total <= account.limit)
            })
            .map_err(|_| FilesystemError::OutOfMemory)?;
        Ok(RdirCharge {
            account: account.clone(),
            bytes,
        })
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
