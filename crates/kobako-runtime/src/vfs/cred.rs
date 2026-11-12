//! Who a call is made for, as Linux's `struct cred`, and the hooks a security
//! module answers, as `security_*` call each registered LSM. The VFS asks them
//! where Linux does and knows nothing of what they decide.

use std::any::Any;
use std::net::SocketAddr;
use std::sync::Arc;

use super::dentry::Dentry;
use super::file::FMode;
use super::inode::Mask;
use super::mount::VfsMount;
use super::openat2::OpenHow;
use super::{Errno, FileType};
use crate::memcg::MemoryBudget;
use crate::network::Connection;
use crate::profile::Profile;

/// What one security module keeps on an open file, as Linux's `f_security`
/// blob holds each module's part; `None` where it keeps nothing.
pub type FileSecurity = Option<Arc<dyn Any + Send + Sync>>;

/// A path a security hook is asked about: the mount it stands on, and its name
/// from that mount's root, built only for a module that reads it, as `struct path`
/// leaves `d_path` to the module that needs one. The name is what
/// `Dentry::path_raw` answers for the entry when the module asks.
#[derive(Clone, Copy)]
pub struct SecurityPath<'a> {
    pub mnt: &'a Arc<VfsMount>,
    pub name: &'a dyn Fn() -> String,
}

/// Answer `hook` for `dentry` on `mnt`, its name read from where the dentry
/// stands when a module asks, as a module builds one from the `struct path` a
/// `security_*` hook is handed.
pub(crate) fn on_path<T>(
    mnt: &Arc<VfsMount>,
    dentry: &Dentry,
    hook: impl FnOnce(SecurityPath<'_>) -> T,
) -> T {
    hook(SecurityPath {
        mnt,
        name: &|| dentry.path_raw(),
    })
}

/// What a security module answers, as a hook in Linux's `lsm_hook_defs.h`. A hook
/// a module does not have allows the call, and a module can only refuse.
pub trait SecurityOperations: Send + Sync {
    /// Whether a call may do what `mask` asks of what `dentry` names, as
    /// `security_inode_permission` is asked once the entry itself allowed it.
    fn inode_permission(&self, _dentry: &Dentry, _mask: Mask) -> Result<(), Errno> {
        Ok(())
    }

    /// Whether a file may be made at `path`, as `security_path_mknod`.
    fn path_mknod(&self, _path: SecurityPath<'_>) -> Result<(), Errno> {
        Ok(())
    }

    /// Whether a directory may be made at `path`, as `security_path_mkdir`.
    fn path_mkdir(&self, _path: SecurityPath<'_>) -> Result<(), Errno> {
        Ok(())
    }

    /// Whether the directory at `path` may go, as `security_path_rmdir`.
    fn path_rmdir(&self, _path: SecurityPath<'_>) -> Result<(), Errno> {
        Ok(())
    }

    /// Whether the file at `path` may go, as `security_path_unlink`.
    fn path_unlink(&self, _path: SecurityPath<'_>) -> Result<(), Errno> {
        Ok(())
    }

    /// Whether a link may be made at `path`, as `security_path_symlink`.
    fn path_symlink(&self, _path: SecurityPath<'_>) -> Result<(), Errno> {
        Ok(())
    }

    /// Whether what `old` names may also go by `new`, as `security_path_link`.
    fn path_link(&self, _old: SecurityPath<'_>, _new: SecurityPath<'_>) -> Result<(), Errno> {
        Ok(())
    }

    /// Whether what `old` names may move to `new`, the dentries of each given, as
    /// `security_path_rename` is handed both.
    fn path_rename(
        &self,
        _old: SecurityPath<'_>,
        _new: SecurityPath<'_>,
        _dentries: (&Arc<Dentry>, &Arc<Dentry>),
    ) -> Result<(), Errno> {
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
    ) -> Result<FileSecurity, Errno> {
        Ok(None)
    }

    /// Whether the open file may be truncated, judged from what this module kept
    /// on it as it opened, as Landlock's `hook_file_truncate` reads the file's
    /// blob.
    fn file_truncate(&self, _kept: &FileSecurity) -> Result<(), Errno> {
        Ok(())
    }

    /// Whether times may be set on the entry at `path`. Linux has no path hook
    /// for times, so this is kobako's own, beside `path_truncate` and
    /// `path_chmod` in kind, letting a module judge the path a call names.
    fn path_utimes(&self, _path: SecurityPath<'_>) -> Result<(), Errno> {
        Ok(())
    }

    /// Whether where the link `dentry` names points may be read, as
    /// `security_inode_readlink` is handed the link.
    fn inode_readlink(&self, _dentry: &Dentry) -> Result<(), Errno> {
        Ok(())
    }

    /// Whether a socket may connect where `connection` names, judged on what
    /// the guest named, as `security_socket_connect`.
    fn socket_connect(&self, _connection: &Connection<'_>) -> Result<(), Errno> {
        Ok(())
    }

    /// Whether a socket may bind `address`, as `security_socket_bind`.
    fn socket_bind(&self, _address: SocketAddr) -> Result<(), Errno> {
        Ok(())
    }
}

/// Who a call is made for, holding the security modules registered for it, as
/// `cred->security` holds a Landlock domain, and the memory budget the host
/// charges what it holds on the caller's behalf to, which every Cred restricted
/// from it shares, as a task's memory cgroup does. Its profile decides what the
/// caller sees of an entry, as `cred->user_ns` decides the owner `cp_new_stat`
/// reports.
pub struct Cred {
    security: Vec<Arc<dyn SecurityOperations>>,
    memory: Arc<MemoryBudget>,
    profile: Profile,
}

/// One no module is registered for, which this crate's tests make calls as.
#[cfg(test)]
impl Default for Cred {
    fn default() -> Self {
        Self {
            security: Vec::new(),
            memory: MemoryBudget::new(None),
            profile: Profile::Hermetic,
        }
    }
}

impl Cred {
    /// One with `security` registered, its memory unbounded.
    pub(crate) fn new(security: Vec<Arc<dyn SecurityOperations>>) -> Arc<Self> {
        Self::within(security, MemoryBudget::new(None))
    }

    /// One with `security` registered, charging what is held on its behalf to
    /// `memory`, seeing entries as the hermetic profile does until told
    /// otherwise.
    pub(crate) fn within(
        security: Vec<Arc<dyn SecurityOperations>>,
        memory: Arc<MemoryBudget>,
    ) -> Arc<Self> {
        Arc::new(Self {
            security,
            memory,
            profile: Profile::Hermetic,
        })
    }

    /// This Cred with `more` modules registered after its own, sharing the
    /// memory held on its behalf, as `landlock_restrict_self` adds a domain.
    pub(crate) fn with_modules(&self, more: Vec<Arc<dyn SecurityOperations>>) -> Arc<Self> {
        let mut security = self.security.clone();
        security.extend(more);
        Arc::new(Self {
            security,
            memory: self.memory.clone(),
            profile: self.profile,
        })
    }

    /// This Cred seeing entries as `profile` does, as a task's credentials
    /// carry the user namespace its stat is reported in.
    pub fn with_profile(&self, profile: Profile) -> Arc<Self> {
        Arc::new(Self {
            security: self.security.clone(),
            memory: self.memory.clone(),
            profile,
        })
    }

    /// The isolation profile the caller runs under.
    pub fn profile(&self) -> Profile {
        self.profile
    }

    /// The budget the caller's memory is charged to, which a Wasm Driver also
    /// charges the guest's own growth to.
    pub fn memory_budget(&self) -> &Arc<MemoryBudget> {
        &self.memory
    }

    /// Ask each module in turn, the first refusal answering, as `call_int_hook`.
    fn call_int_hook(
        &self,
        hook: impl Fn(&dyn SecurityOperations) -> Result<(), Errno>,
    ) -> Result<(), Errno> {
        self.security
            .iter()
            .try_for_each(|module| hook(module.as_ref()))
    }

    pub(crate) fn security_inode_permission(
        &self,
        dentry: &Dentry,
        mask: Mask,
    ) -> Result<(), Errno> {
        self.call_int_hook(|module| module.inode_permission(dentry, mask))
    }

    pub(crate) fn security_path_mknod(&self, path: SecurityPath<'_>) -> Result<(), Errno> {
        self.call_int_hook(|module| module.path_mknod(path))
    }

    pub(crate) fn security_path_mkdir(&self, path: SecurityPath<'_>) -> Result<(), Errno> {
        self.call_int_hook(|module| module.path_mkdir(path))
    }

    pub(crate) fn security_path_rmdir(&self, path: SecurityPath<'_>) -> Result<(), Errno> {
        self.call_int_hook(|module| module.path_rmdir(path))
    }

    pub(crate) fn security_path_unlink(&self, path: SecurityPath<'_>) -> Result<(), Errno> {
        self.call_int_hook(|module| module.path_unlink(path))
    }

    pub(crate) fn security_path_symlink(&self, path: SecurityPath<'_>) -> Result<(), Errno> {
        self.call_int_hook(|module| module.path_symlink(path))
    }

    pub(crate) fn security_path_link(
        &self,
        old: SecurityPath<'_>,
        new: SecurityPath<'_>,
    ) -> Result<(), Errno> {
        self.call_int_hook(|module| module.path_link(old, new))
    }

    pub(crate) fn security_path_rename(
        &self,
        old: SecurityPath<'_>,
        new: SecurityPath<'_>,
        dentries: (&Arc<Dentry>, &Arc<Dentry>),
    ) -> Result<(), Errno> {
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
    ) -> Result<Vec<FileSecurity>, Errno> {
        self.security
            .iter()
            .map(|module| module.file_open(path, kind, mode, how))
            .collect()
    }

    /// Ask each module whether the file may be truncated, handing each what it
    /// kept on the file as it opened.
    pub(crate) fn security_file_truncate(&self, kept: &[FileSecurity]) -> Result<(), Errno> {
        self.security
            .iter()
            .zip(kept)
            .try_for_each(|(module, kept)| module.file_truncate(kept))
    }

    pub(crate) fn security_path_utimes(&self, path: SecurityPath<'_>) -> Result<(), Errno> {
        self.call_int_hook(|module| module.path_utimes(path))
    }

    pub(crate) fn security_inode_readlink(&self, dentry: &Dentry) -> Result<(), Errno> {
        self.call_int_hook(|module| module.inode_readlink(dentry))
    }

    pub(crate) fn security_socket_connect(&self, connection: &Connection<'_>) -> Result<(), Errno> {
        self.call_int_hook(|module| module.socket_connect(connection))
    }

    pub(crate) fn security_socket_bind(&self, address: SocketAddr) -> Result<(), Errno> {
        self.call_int_hook(|module| module.socket_bind(address))
    }

    /// Charge `bytes` more held on this caller's behalf, refusing them past its
    /// budget, as `security_vm_enough_memory_mm` ends in `__vm_enough_memory`.
    pub(crate) fn security_vm_enough_memory(&self, bytes: usize) -> Result<(), Errno> {
        self.memory.try_charge(bytes)
    }

    /// `bytes` charged earlier are held no more, as `vm_unacct_memory` gives
    /// them back.
    pub(crate) fn vm_unacct_memory(&self, bytes: usize) {
        self.memory.uncharge(bytes);
    }
}
