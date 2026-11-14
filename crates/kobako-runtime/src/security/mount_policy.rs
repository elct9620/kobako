//! A Mount's Policy as a security module, answering the path hooks for that
//! Mount alone, as Landlock's `fs.c` answers the filesystem rights of its
//! domain.

use std::sync::Arc;

use super::{FileSecurity, SecurityOperations, SecurityPath};
use crate::mount::{Access, Mount, MountRequest};
use crate::policy::Policy;
use crate::vfs::{Dentry, FMode, FileType, OpenHow, VfsMount, PATH_MAX};
use crate::Errno;

/// What a Mount's Policy allowed a file as it opened, kept on the open file, as
/// Landlock's file blob keeps `allowed_access`.
struct AllowedAccess(Access);

/// A Mount's Policy, answering only for that Mount.
pub(super) struct MountPolicy {
    mnt: Arc<VfsMount>,
    at: String,
    policy: Arc<dyn for<'a> Policy<MountRequest<'a>>>,
}

impl MountPolicy {
    /// `mount`'s Policy, when it has one.
    pub(super) fn of(mount: &Mount) -> Option<Self> {
        Some(Self {
            mnt: mount.mnt().clone(),
            at: mount.at().to_string(),
            policy: mount.policy()?.clone(),
        })
    }

    fn guards(&self, mnt: &Arc<VfsMount>) -> bool {
        Arc::ptr_eq(&self.mnt, mnt)
    }

    /// The absolute guest path `path`, from the Mount's root, stands for.
    fn guest_path(&self, path: &str) -> String {
        match (self.at.as_str(), path) {
            (at, "/") => at.to_string(),
            ("/", path) => path.to_string(),
            (at, path) => format!("{at}{path}"),
        }
    }

    /// Judge `path` as AppArmor's `aa_path_perm` judges a whole path once, each
    /// hook below reaching it as AppArmor's do. The Policy judges the path as the
    /// Filesystem stores its names, so a host folding case or Unicode form cannot
    /// hand it one spelling of an entry it refuses by another. One a Policy could
    /// not be handed within `PATH_MAX` is refused as too long, as AppArmor's
    /// `aa_path_name` refuses one past its buffer.
    fn path_perm(&self, path: SecurityPath<'_>, access: Access) -> Result<(), Errno> {
        if !self.guards(path.mnt) {
            return Ok(());
        }
        let path = self.guest_path(&(path.name)());
        if path.len() >= PATH_MAX {
            return Err(Errno::InvalidFilename);
        }
        match self.policy.allows(&MountRequest {
            path: &path,
            access,
        }) {
            true => Ok(()),
            false => Err(Errno::PermissionDenied),
        }
    }
}

impl SecurityOperations for MountPolicy {
    fn path_mknod(&self, path: SecurityPath<'_>) -> Result<(), Errno> {
        self.path_perm(path, Access::ReadWrite)
    }

    fn path_mkdir(&self, path: SecurityPath<'_>) -> Result<(), Errno> {
        self.path_perm(path, Access::ReadWrite)
    }

    fn path_rmdir(&self, path: SecurityPath<'_>) -> Result<(), Errno> {
        self.path_perm(path, Access::ReadWrite)
    }

    fn path_unlink(&self, path: SecurityPath<'_>) -> Result<(), Errno> {
        self.path_perm(path, Access::ReadWrite)
    }

    fn path_symlink(&self, path: SecurityPath<'_>) -> Result<(), Errno> {
        self.path_perm(path, Access::ReadWrite)
    }

    fn path_link(&self, old: SecurityPath<'_>, new: SecurityPath<'_>) -> Result<(), Errno> {
        self.path_perm(old, Access::ReadWrite)?;
        self.path_perm(new, Access::ReadWrite)
    }

    /// Asked once for each open, for what the open may change, and kept on the
    /// file, as Landlock keeps the rights it allowed. A file the open made was
    /// judged as a change already, through `path_mknod`.
    fn file_open(
        &self,
        path: SecurityPath<'_>,
        _kind: FileType,
        mode: FMode,
        how: &OpenHow,
    ) -> Result<FileSecurity, Errno> {
        if !self.guards(path.mnt) {
            return Ok(None);
        }
        let created = mode.contains(FMode::CREATED);
        let access = match created || mode.contains(FMode::WRITE) || how.truncate {
            true => Access::ReadWrite,
            false => Access::Read,
        };
        if !created {
            self.path_perm(path, access)?;
        }
        Ok(Some(Arc::new(AllowedAccess(access))))
    }

    /// A truncate follows what the Policy allowed as the file opened, wherever
    /// the file stands now, as Landlock's `hook_file_truncate` reads the file's
    /// rights.
    fn file_truncate(&self, kept: &FileSecurity) -> Result<(), Errno> {
        match kept
            .as_deref()
            .and_then(|kept| kept.downcast_ref::<AllowedAccess>())
        {
            Some(AllowedAccess(Access::Read)) => Err(Errno::PermissionDenied),
            _ => Ok(()),
        }
    }

    fn path_utimes(&self, path: SecurityPath<'_>) -> Result<(), Errno> {
        self.path_perm(path, Access::ReadWrite)
    }

    /// Both paths are judged. Everything beneath a directory would take paths the
    /// Policy never judged, and a Policy answers for one path, not a tree, so its
    /// move is refused as one across devices and the guest copies entry by entry
    /// instead, as Landlock answers a move it cannot show keeps each right beneath.
    /// A move onto its own name takes no new path.
    fn path_rename(
        &self,
        old: SecurityPath<'_>,
        new: SecurityPath<'_>,
        (old_dentry, new_dentry): (&Arc<Dentry>, &Arc<Dentry>),
    ) -> Result<(), Errno> {
        self.path_perm(old, Access::ReadWrite)?;
        self.path_perm(new, Access::ReadWrite)?;
        let moves_dir =
            old_dentry.kind() == Ok(FileType::Dir) && !Arc::ptr_eq(old_dentry, new_dentry);
        match moves_dir && self.guards(old.mnt) {
            true => Err(Errno::CrossesDevices),
            false => Ok(()),
        }
    }
}
