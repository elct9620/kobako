//! The security modules an Invocation registers: kobako's own rules, and the
//! Host App's Policy for each Mount that has one. Linux has neither, so they
//! answer through the hooks it leaves for such modules, and the VFS never
//! learns what they decide.

use std::sync::Arc;

use crate::mount::{Mount, Policy};
use crate::vfs::{
    Access, Cred, Denial, Error, FileType, FilesystemError, Lookup, Path, SecurityOperations,
    VfsMount, PATH_MAX,
};

/// What every Invocation holds to, whatever its Mounts allow.
struct KobakoSecurity;

impl SecurityOperations for KobakoSecurity {
    /// Only a file or a directory opens: the runtime provides no peer for a
    /// FIFO or a socket, and a device would act on being opened.
    fn file_open(&self, kind: FileType) -> Result<(), FilesystemError> {
        match kind {
            FileType::File | FileType::Directory => Ok(()),
            _ => Err(FilesystemError::PermissionDenied),
        }
    }

    /// A guest never reads where a link points.
    fn inode_readlink(&self) -> Result<(), FilesystemError> {
        Err(FilesystemError::Unsupported)
    }

    /// Where a walk lands stepping out of a link tells where the link points.
    fn path_step_out_of_link(&self) -> Result<(), FilesystemError> {
        Err(FilesystemError::FilesystemLoop)
    }

    /// Which name a file made through a link appears at tells where the link
    /// points.
    fn path_mknod(&self, followed_link: bool) -> Result<(), FilesystemError> {
        match followed_link {
            true => Err(FilesystemError::FilesystemLoop),
            false => Ok(()),
        }
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
    /// entry it refuses by another. One a Policy could not be handed within
    /// `PATH_MAX` is refused as too long, as AppArmor's `aa_path_name` refuses
    /// one past its buffer.
    fn path_perm(
        &self,
        mnt: &Arc<VfsMount>,
        path: &dyn Fn() -> String,
        access: Access,
    ) -> Result<(), Error> {
        if !self.guards(mnt) {
            return Ok(());
        }
        let path = self.guest_path(&path());
        if path.len() >= PATH_MAX {
            return Err(FilesystemError::InvalidFilename.into());
        }
        match self.policy.allows(&path, access) {
            true => Ok(()),
            false => Err(Denial::Refused.into()),
        }
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
        let sb = path.mnt().sb();
        let dentries = sb.dentries();
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
        let mut dir = sb.root().clone();
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
    use crate::filesystem::Tmpfs;

    /// A Policy allowing everything, counting how often it is asked.
    #[derive(Default)]
    struct Counts(std::sync::atomic::AtomicUsize);

    impl Policy for Counts {
        fn allows(&self, _path: &str, _access: Access) -> bool {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            true
        }
    }

    /// What making an entry answers through a directory moved past
    /// `PATH_MAX`, beneath a mount `prepare` finishes.
    fn made_past_the_limit(prepare: impl FnOnce(Mount) -> Mount) -> Result<(), Error> {
        let (deep, seed) = (vec!["a"; 2000].join("/"), "x");
        let tmpfs: Tmpfs = [
            (format!("c0/{deep}/file"), seed),
            (format!("c1/{deep}/file"), seed),
        ]
        .into_iter()
        .collect();
        let mount = Mount::new(
            "/data",
            crate::filesystem::Filesystem::new(Arc::new(tmpfs)),
            Access::ReadWrite,
        )
        .expect("/data is a guest path");
        let mount = prepare(mount);
        let cred = prepare_creds([&mount]);
        let root = Path::root(mount.mnt());
        let how = crate::vfs::OpenHow {
            directory: true,
            ..Default::default()
        };
        let held = crate::vfs::do_file_open(&cred, &root, &format!("c0/{deep}"), &how)
            .unwrap_or_else(|_| panic!("the directory must open"));
        crate::vfs::filename_renameat2(
            &Arc::default(),
            &root,
            "c0",
            &root,
            &format!("c1/{deep}/z"),
        )
        .expect("the move must land");
        crate::vfs::filename_mkdirat(&cred, held.path(), "made")
    }

    // Linux's walk sets no limit on where a directory stands, so a walk
    // without a Policy has no reason to.
    // @behavior RT-203
    #[test]
    fn a_path_past_the_limit_is_made_where_no_policy_needs_it() {
        assert_eq!(
            made_past_the_limit(|mount| mount),
            Ok(()),
            "vfs::filename_mkdirat through a directory moved past PATH_MAX, beneath a Mount with \
             no Policy, must make the entry"
        );
    }

    // @behavior RT-204
    #[test]
    fn a_path_a_policy_cannot_be_handed_is_too_long() {
        let policy = Arc::new(Counts::default());
        let asked = policy.clone();
        let made = made_past_the_limit(move |mount| mount.with_policy(policy));
        let asked_past_the_limit = asked.0.load(std::sync::atomic::Ordering::SeqCst);
        assert_eq!(
            (made, asked_past_the_limit),
            (Err(FilesystemError::InvalidFilename.into()), 1),
            "vfs::filename_mkdirat through a directory moved past PATH_MAX, beneath a Policy, \
             must answer ENAMETOOLONG, the Policy asked only of the open before the move"
        );
    }

    // With no security module a walk answers as Linux's, so the rules against
    // stepping out of a link and making a file through one are this module's
    // and no part of the VFS.
    // @behavior RT-214
    #[cfg(unix)]
    #[test]
    fn a_link_answers_as_linuxs_where_no_module_refuses() {
        use crate::vfs::{do_file_open, vfs_fstatat, OpenHow};
        let host = std::env::temp_dir().join(format!("kobako-link-rules-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&host);
        std::fs::create_dir_all(host.join("dir/sub")).expect("the host tree must be made");
        std::os::unix::fs::symlink("dir/sub", host.join("link")).expect("the link must be made");
        std::os::unix::fs::symlink("dir/made", host.join("dangling"))
            .expect("the link must be made");
        let filesystem = crate::filesystem::Filesystem::new(Arc::new(
            crate::filesystem::Bind::new(&host).expect("the host directory must open"),
        ));
        let mount = Mount::new("/data", filesystem, Access::ReadWrite).expect("a guest path");
        let root = Path::root(mount.mnt());
        let create = OpenHow {
            write: true,
            create: true,
            follow: true,
            ..OpenHow::default()
        };
        let answers = |cred: &Arc<Cred>| {
            (
                vfs_fstatat(cred, &root, "link/..", false).map(|_| ()),
                vfs_fstatat(cred, &root, "link/.", false).map(|_| ()),
                do_file_open(cred, &root, "dangling", &create).map(|_| ()),
            )
        };
        let refused = answers(&prepare_creds([&mount]));
        let linuxs = answers(&Arc::default());
        let made = host.join("dir/made").exists();
        let _ = std::fs::remove_dir_all(&host);
        let looped = || Err(FilesystemError::FilesystemLoop.into());
        assert_eq!(
            (refused, linuxs, made),
            (
                (looped(), looped(), looped()),
                (Ok(()), Ok(()), Ok(())),
                true
            ),
            "`..` and `.` out of a link and a create through a dangling one must answer ELOOP \
             under KobakoSecurity, and resolve as Linux's where no module refuses"
        );
    }

    // A Bind refuses these itself, so only asking the rule directly shows it
    // holds for a Filesystem that would open them.
    // @behavior RT-197
    #[test]
    fn only_a_file_or_a_directory_opens() {
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
            [true, true, false, false, false, false, false, false],
            "KobakoSecurity::file_open must pass a file and a directory, and refuse every other \
             kind"
        );
    }
}
