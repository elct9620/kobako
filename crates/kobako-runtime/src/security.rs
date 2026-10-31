//! The security modules an Invocation registers: kobako's own rules, and the
//! Host App's Policy for each Mount that has one. Linux has neither, so they
//! answer through the hooks it leaves for such modules, and the VFS never
//! learns what they decide.

use std::sync::Arc;

use crate::filesystem::{FileType, FilesystemError, Lookup};
use crate::mount::{Mount, Policy};
use crate::vfs::{Access, Cred, Denial, Error, Path, SecurityOperations, VfsMount};

/// What every Invocation holds to, whatever its Mounts allow.
struct KobakoSecurity;

impl SecurityOperations for KobakoSecurity {
    /// Only a file or a directory opens: the runtime provides no peer for a
    /// FIFO or a socket, and a device would act on being opened.
    fn file_open(&self, kind: FileType) -> Result<(), FilesystemError> {
        match kind {
            FileType::File | FileType::Directory | FileType::Symlink => Ok(()),
            _ => Err(FilesystemError::PermissionDenied),
        }
    }

    /// A guest never reads where a link points.
    fn inode_readlink(&self) -> Result<(), FilesystemError> {
        Err(FilesystemError::Unsupported)
    }
}

/// A Mount's Policy, answering only for that Mount.
struct MountPolicy {
    mnt: Arc<VfsMount>,
    at: String,
    policy: Arc<dyn Policy>,
}

impl MountPolicy {
    fn guards(&self, mnt: &Arc<VfsMount>) -> bool {
        Arc::ptr_eq(&self.mnt, mnt)
    }

    /// The absolute guest path `path`, relative to the Mount's root, stands
    /// for.
    fn guest_path(&self, path: &str) -> String {
        match (self.at.as_str(), path) {
            (at, "") => at.to_string(),
            ("/", path) => format!("/{path}"),
            (at, path) => format!("{at}/{path}"),
        }
    }
}

impl SecurityOperations for MountPolicy {
    /// The Policy judges the path as the Filesystem stores its names, so a
    /// host folding case or Unicode form cannot hand it one spelling of an
    /// entry it refuses by another.
    fn inode_permission(
        &self,
        mnt: &Arc<VfsMount>,
        path: &dyn Fn() -> String,
        access: Access,
    ) -> Result<(), Denial> {
        if !self.guards(mnt) || self.policy.allows(&self.guest_path(&path()), access) {
            return Ok(());
        }
        Err(Denial::Refused)
    }

    /// A link leads to a path the Policy never judged, so the call is refused
    /// as one past too many links.
    fn inode_follow_link(&self, mnt: &Arc<VfsMount>) -> Result<(), FilesystemError> {
        match self.guards(mnt) {
            true => Err(FilesystemError::FilesystemLoop),
            false => Ok(()),
        }
    }

    /// Everything beneath a directory would take paths the Policy never
    /// judged, so its move is refused as one across devices and the guest
    /// copies entry by entry instead, as Landlock refuses such a move.
    fn path_rename(&self, mnt: &Arc<VfsMount>, is_dir: bool) -> Result<(), FilesystemError> {
        match is_dir && self.guards(mnt) {
            true => Err(FilesystemError::CrossesDevices),
            false => Ok(()),
        }
    }

    /// Refuse a call through a directory unless its dentry still names what
    /// the runtime opened there, so the Policy judges the path it stands at.
    /// A dentry dropped or removed is refused as Linux's AppArmor refuses an
    /// unlinked one; one held is asked of the directory above through
    /// `revalidate`, walking there from the root as gVisor's `revalidatePath`
    /// walks; and one whose identity the runtime never learned cannot be told
    /// from another, so it is refused too. Linux asks nothing of the dentry a
    /// walk starts from; this keeps a host or another Filesystem instance
    /// moving a directory behind the runtime's back from having a Policy judge
    /// a path it no longer stands at.
    fn path_walk(&self, path: &Path) -> Result<(), Error> {
        if !self.guards(path.mnt()) {
            return Ok(());
        }
        let dentry = path.dentry();
        let Some(parent) = dentry.parent() else {
            return Ok(());
        };
        let filesystem = path.mnt().filesystem();
        let dentries = filesystem.dentries();
        let told_apart = dentry
            .inode()
            .is_some_and(|inode| inode.identity().is_some());
        if dentries.unlinked(dentry) || !told_apart {
            return Err(FilesystemError::NotFound.into());
        }
        let Some(ops) = dentry.ops() else {
            return Ok(());
        };
        let mut above = Vec::new();
        let mut at = Some(parent);
        while let Some(next) = at {
            at = next.parent();
            above.push(next);
        }
        let mut dir = filesystem.root().clone();
        for ancestor in above.iter().rev().skip(1) {
            dir = match dir.lookup(ancestor) {
                Some(Ok(Lookup::Found(inode))) if inode.kind() == FileType::Directory => {
                    inode.ops().clone()
                }
                Some(Err(err))
                    if !matches!(
                        err,
                        FilesystemError::NotFound | FilesystemError::NotADirectory
                    ) =>
                {
                    return Err(err.into())
                }
                _ => return Err(FilesystemError::NotFound.into()),
            };
        }
        if ops.revalidate(dir.as_ref(), dentry)? {
            return Ok(());
        }
        dentries.invalidate(dentry);
        Err(FilesystemError::NotFound.into())
    }
}

/// The Cred an Invocation over `mounts` runs with, as `prepare_creds` makes
/// one for a task: kobako's own rules, then each Mount's Policy.
pub fn prepare_creds<'a>(mounts: impl IntoIterator<Item = &'a Mount>) -> Arc<Cred> {
    let mut security: Vec<Arc<dyn SecurityOperations>> = vec![Arc::new(KobakoSecurity)];
    for mount in mounts {
        if let Some(policy) = mount.policy() {
            security.push(Arc::new(MountPolicy {
                mnt: mount.mnt().clone(),
                at: mount.at().to_string(),
                policy: policy.clone(),
            }));
        }
    }
    Cred::new(security)
}

#[cfg(test)]
mod tests {
    use super::*;

    // A Bind refuses these itself, so only asking the rule directly shows it
    // holds for a Filesystem that would open them.
    // @behavior RT-197
    #[test]
    fn only_a_file_a_directory_or_a_link_opens() {
        let judged = |kind| KobakoSecurity.file_open(kind).is_ok();
        assert_eq!(
            [
                FileType::File,
                FileType::Directory,
                FileType::Symlink,
                FileType::Fifo,
                FileType::Socket,
                FileType::BlockDevice,
                FileType::CharDevice,
                FileType::Unknown,
            ]
            .map(judged),
            [true, true, true, false, false, false, false, false],
            "KobakoSecurity::file_open must pass a file, a directory, and a link, and refuse \
             every other kind"
        );
    }
}
