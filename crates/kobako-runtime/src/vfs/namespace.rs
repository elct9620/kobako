//! Which mount stands on which dentry beneath one root, as Linux's mount
//! namespace holds the tree `__lookup_mnt` answers from, so a walk reaching a
//! mount point by any spelling or through any link steps onto what is mounted
//! there.

use std::sync::Arc;

use super::dentry::Dentry;
use super::mount::VfsMount;
use super::namei::{filename_mkdirat, walk_mountpoint};
use super::path::Path;
use crate::cred::Cred;
use crate::filesystem::{FileType, Filesystem, Tmpfs};
use crate::mount::UnplacedMountError;
use crate::Errno;

/// One mount standing on a dentry of another, as Linux's `struct mount` keeps
/// `mnt_parent` and `mnt_mountpoint`. The mount point is held while the mount
/// stands, so a walk finds that one dentry and no other.
struct Mounted {
    parent: Arc<VfsMount>,
    mountpoint: Arc<Dentry>,
    mnt: Arc<VfsMount>,
}

impl Drop for Mounted {
    fn drop(&mut self) {
        self.mountpoint.unpin_mountpoint();
    }
}

/// Where a mount is to stand, as Linux's `pinned_mountpoint` holds the parent
/// and the mount point.
struct PinnedMountpoint {
    parent: Arc<VfsMount>,
    mountpoint: Arc<Dentry>,
}

/// The mounts one Invocation reaches, each standing on a dentry beneath one
/// root, as Linux's `mnt_namespace` holds its `root` and links each mount to
/// its parent.
#[derive(Default, Clone)]
pub(crate) struct MntNamespace {
    root: Option<Arc<VfsMount>>,
    mounted: Vec<Arc<Mounted>>,
}

impl MntNamespace {
    /// The tree the mounts at their absolute guest paths make beneath one
    /// root: the mount at `/`, or else a read-only root holding only the
    /// directories leading to each mount, as a container runtime makes its
    /// mount points; none where no mount is made. Each is placed as Linux's
    /// `mount` places it: its path walked from the root, through the mounts
    /// placed before it, never through a link, as `LOOKUP_NO_SYMLINKS` refuses
    /// one. Shallower paths go first, as a container runtime mounts a parent
    /// before its children, and of two landing where one stands, the later
    /// stands over the earlier and hides it. A mount Linux's `mount` refuses,
    /// its walk failing or landing on no directory, refuses the whole tree, as
    /// a container fails to start.
    pub(crate) fn new<'a>(
        cred: &Arc<Cred>,
        mounts: impl IntoIterator<Item = (&'a str, &'a Arc<VfsMount>)>,
    ) -> Result<Self, UnplacedMountError> {
        let mut mounts: Vec<_> = mounts.into_iter().collect();
        mounts.sort_by_key(|(at, _)| at.split('/').filter(|name| !name.is_empty()).count());
        let mut namespace = Self {
            root: match mounts.first() {
                None => None,
                Some(&("/", root)) => Some(Arc::clone(root)),
                Some(_) => Some(init_rootfs(cred, mounts.iter().map(|(at, _)| *at))),
            },
            mounted: Vec::new(),
        };
        let Some(root) = namespace.root.clone() else {
            return Ok(namespace);
        };
        for &(at, mnt) in mounts.iter().filter(|(at, _)| *at != "/") {
            let path = at.trim_start_matches('/');
            let walker = cred.with_mnt_ns(Arc::new(namespace.clone()));
            let root = Path::root(&namespace.topmost_overmount(&root));
            let pinned = namespace
                .where_to_mount(&walker, &root, path)
                .map_err(|errno| UnplacedMountError {
                    at: at.to_string(),
                    errno,
                })?;
            namespace.mount(pinned, mnt);
        }
        Ok(namespace)
    }

    /// Stand `mnt` on `pinned`, marking its dentry a mount point.
    fn mount(&mut self, pinned: PinnedMountpoint, mnt: &Arc<VfsMount>) {
        pinned.mountpoint.d_set_mounted();
        self.mounted.push(Arc::new(Mounted {
            parent: pinned.parent,
            mountpoint: pinned.mountpoint,
            mnt: Arc::clone(mnt),
        }));
    }

    /// Where a mount at `path` beneath `root` stands as `cred` walks it, as
    /// `do_mount` looks its target up and `where_to_mount` stacks it: on the
    /// dentry the walk lands on, which on a mounted name is the root of the
    /// topmost mount there, so the new one stands over it. A walk that fails
    /// answers its error, and one landing on no directory ENOTDIR, as
    /// `graft_tree` refuses it.
    fn where_to_mount(
        &self,
        cred: &Arc<Cred>,
        root: &Path,
        path: &str,
    ) -> Result<PinnedMountpoint, Errno> {
        let nd = walk_mountpoint(cred, root, path)?;
        let dentry = nd.path()?;
        if dentry.file_type() != Ok(FileType::Dir) {
            return Err(Errno::NotADirectory);
        }
        Ok(PinnedMountpoint {
            parent: nd.mnt.clone(),
            mountpoint: dentry,
        })
    }

    /// The mount every guest path starts from, as `mnt_namespace` holds its
    /// `root`; none where no mount is made.
    pub(crate) fn root(&self) -> Option<&Arc<VfsMount>> {
        self.root.as_ref()
    }

    /// The mount standing over the root of `mnt`, and over that one's, to the
    /// last, as Linux's `topmost_overmount` follows each `overmount`; `mnt`
    /// itself when none does.
    pub(crate) fn topmost_overmount(&self, mnt: &Arc<VfsMount>) -> Arc<VfsMount> {
        let mut top = Arc::clone(mnt);
        while let Some(over) = self.__lookup_mnt(&top, top.root()) {
            top = over;
        }
        top
    }

    /// The mount standing on `dentry` beneath `mnt`, as Linux's `__lookup_mnt`.
    /// One stands there at most, as a later mount stacks on its root.
    pub(crate) fn __lookup_mnt(
        &self,
        mnt: &Arc<VfsMount>,
        dentry: &Arc<Dentry>,
    ) -> Option<Arc<VfsMount>> {
        self.mounted
            .iter()
            .find(|child| Arc::ptr_eq(&child.parent, mnt) && Arc::ptr_eq(&child.mountpoint, dentry))
            .map(|child| Arc::clone(&child.mnt))
    }

    /// Whether a mount of this namespace pins `dentry`, its path leading
    /// through it: the mount stands on it or beneath it. A removal or a move
    /// asks it where Linux asks `is_local_mountpoint`: each Invocation places a
    /// mount again by its path, so a pinned name going would open what the
    /// mount hides to the next.
    pub(crate) fn pins(&self, dentry: &Arc<Dentry>) -> bool {
        self.mounted
            .iter()
            .any(|child| child.mountpoint.is_subdir(dentry))
    }

    /// Whether any mount stands beneath `mnt`, as Linux finds its `mnt_mounts`
    /// not empty.
    pub(crate) fn has_children(&self, mnt: &Arc<VfsMount>) -> bool {
        self.mounted
            .iter()
            .any(|child| Arc::ptr_eq(&child.parent, mnt))
    }
}

/// A read-only root holding the directory at each of `paths` and those above
/// it, made as `cred`, as Linux's `init_mount_tree` mounts a rootfs and
/// `init_mkdir` makes the directories a mount stands on. A directory two paths
/// share is made once.
fn init_rootfs<'a>(cred: &Arc<Cred>, paths: impl IntoIterator<Item = &'a str>) -> Arc<VfsMount> {
    let sb = Filesystem::new(Tmpfs::new()).expect("an empty Tmpfs stands");
    let made = Path::root(&VfsMount::kern_mount(Arc::clone(&sb)));
    for path in paths {
        let mut above = String::new();
        for name in path.split('/').filter(|name| !name.is_empty()) {
            above.push('/');
            above.push_str(name);
            match filename_mkdirat(cred, &made, above.trim_start_matches('/')) {
                Ok(()) | Err(Errno::AlreadyExists) => {}
                Err(errno) => unreachable!("an empty Tmpfs makes {above}: {errno:?}"),
            }
        }
    }
    VfsMount::new(sb, true)
}
