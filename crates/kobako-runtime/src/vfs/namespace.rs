//! Which mount stands on which dentry, as Linux's mount namespace holds the
//! tree `__lookup_mnt` answers from, so a walk reaching a mount point by any
//! spelling or through any link steps onto what is mounted there.

use std::sync::Arc;

use super::dentry::Dentry;
use super::mount::VfsMount;
use super::namei::{filename_lookup, filename_parentat, lookup_dentry};
use super::path::Path;
use crate::cred::Cred;
use crate::filesystem::FileType;
use crate::mount::{outermost_holder_of, UnplacedMountError};
use crate::Errno;

/// One mount standing on a dentry of another, as Linux's `struct mount` keeps
/// `mnt_parent` and `mnt_mountpoint`. The mount point is held while the mount
/// stands, so a walk finds that one dentry and no other. The dentries its path
/// led through are kept too, as each Invocation places it by that path again.
struct Mounted {
    parent: Arc<VfsMount>,
    mountpoint: Arc<Dentry>,
    mnt: Arc<VfsMount>,
    trail: Vec<Arc<Dentry>>,
}

impl Drop for Mounted {
    fn drop(&mut self) {
        self.mountpoint.unpin_mountpoint();
    }
}

/// Where a mount is to stand: the mount and dentry it stands on, and each
/// dentry its path led through.
struct Spot {
    parent: Arc<VfsMount>,
    mountpoint: Arc<Dentry>,
    trail: Vec<Arc<Dentry>>,
}

/// The mounts one Invocation reaches beneath the outermost ones, each standing
/// on a dentry as Linux's `mnt_namespace` links each mount to its parent.
#[derive(Default, Clone)]
pub(crate) struct MntNamespace {
    mounted: Vec<Arc<Mounted>>,
}

impl MntNamespace {
    /// The tree the mounts at their absolute guest paths make, each placed as
    /// Linux's `mount` places it: its path walked from the root of the
    /// outermost mount holding it, through the mounts placed before it,
    /// following a link at its last name too, as `do_mount` looks its target
    /// up with `LOOKUP_FOLLOW`, never above that root. Shallower paths go
    /// first, as a container runtime mounts a parent before its children, and
    /// of two landing where one stands, the later stands over the earlier and
    /// hides it. The last name may stand for nothing, which Linux never lets a
    /// mount do, so the parent's name is hidden even once made. A mount
    /// Linux's `mount` refuses, its walk failing or landing on no directory,
    /// refuses the whole tree, as a container fails to start.
    pub(crate) fn new<'a>(
        cred: &Arc<Cred>,
        mounts: impl IntoIterator<Item = (&'a str, &'a Arc<VfsMount>)>,
    ) -> Result<Self, UnplacedMountError> {
        let mut mounts: Vec<_> = mounts.into_iter().collect();
        mounts.sort_by_key(|(at, _)| at.split('/').filter(|name| !name.is_empty()).count());
        let mut namespace = Self::default();
        for &(at, mnt) in &mounts {
            let Some((root_at, root)) = outermost_holder_of(at, mounts.iter().copied()) else {
                continue;
            };
            let path = at[root_at.len()..].trim_start_matches('/');
            let walker = cred.with_mnt_ns(Arc::new(namespace.clone()));
            let root = Path::root(&namespace.topmost_overmount(root));
            let spot = namespace
                .where_to_mount(&walker, &root, path)
                .map_err(|errno| UnplacedMountError {
                    at: at.to_string(),
                    errno,
                })?;
            namespace.mount(spot, mnt);
        }
        Ok(namespace)
    }

    /// Stand `mnt` on `spot`, marking its dentry a mount point.
    fn mount(&mut self, spot: Spot, mnt: &Arc<VfsMount>) {
        spot.mountpoint.d_set_mounted();
        self.mounted.push(Arc::new(Mounted {
            parent: spot.parent,
            mountpoint: spot.mountpoint,
            mnt: Arc::clone(mnt),
            trail: spot.trail,
        }));
    }

    /// Where a mount at `path` beneath `root` stands as `cred` walks it, as
    /// `do_mount` looks its target up and `where_to_mount` stacks it: on the
    /// dentry the walk lands on, which on a mounted name is the root of the
    /// topmost mount there, so the new one stands over it. A name standing for
    /// nothing is stood on as it is. A walk that fails answers its error, and
    /// one landing on no directory ENOTDIR, as `graft_tree` refuses it.
    fn where_to_mount(&self, cred: &Arc<Cred>, root: &Path, path: &str) -> Result<Spot, Errno> {
        let nd = match filename_lookup(cred, root, path, true) {
            Ok(nd) => nd,
            Err(errno) => {
                let nd = filename_parentat(cred, root, path)?;
                let dentry = lookup_dentry(nd.mnt.sb(), nd.dentry(), &nd.last)?;
                return match dentry.inode() {
                    None => Ok(Spot {
                        parent: nd.mnt.clone(),
                        mountpoint: dentry,
                        trail: nd.trail,
                    }),
                    Some(_) => Err(errno),
                };
            }
        };
        let dentry = nd.path()?;
        if dentry.file_type() != Ok(FileType::Dir) {
            return Err(Errno::NotADirectory);
        }
        Ok(Spot {
            parent: nd.mnt.clone(),
            mountpoint: dentry,
            trail: nd.trail,
        })
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

    /// Whether the path of a mount of this namespace leads through `dentry`:
    /// the mount stands on it or beneath it, or the walk placing the mount
    /// stepped onto it or beneath it. A removal or a move asks it where Linux
    /// asks `is_local_mountpoint`: each Invocation places a mount again by its
    /// path, so such a name going would open what the mount hides to the next.
    pub(crate) fn is_on_mount_path(&self, dentry: &Arc<Dentry>) -> bool {
        self.mounted.iter().any(|child| {
            child.mountpoint.is_subdir(dentry)
                || child.trail.iter().any(|stepped| stepped.is_subdir(dentry))
        })
    }

    /// Whether any mount stands beneath `mnt`, as Linux finds its `mnt_mounts`
    /// not empty.
    pub(crate) fn has_children(&self, mnt: &Arc<VfsMount>) -> bool {
        self.mounted
            .iter()
            .any(|child| Arc::ptr_eq(&child.parent, mnt))
    }
}
