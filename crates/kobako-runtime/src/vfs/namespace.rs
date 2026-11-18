//! Which mount stands on which dentry, as Linux's mount namespace holds the
//! tree `__lookup_mnt` answers from, so a walk reaching a mount point by any
//! spelling, through any link, or after a directory above it moved steps onto
//! what is mounted there.

use std::sync::Arc;

use super::dentry::Dentry;
use super::mount::VfsMount;
use super::namei::{filename_lookup, filename_parentat, lookup_dentry};
use super::path::Path;
use crate::cred::Cred;
use crate::filesystem::FileType;
use crate::mount::{outermost_holder_of, UnplacedMount};
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
    ) -> Result<Self, UnplacedMount> {
        let mut mounts: Vec<_> = mounts.into_iter().collect();
        mounts.sort_by_key(|(at, _)| at.split('/').filter(|name| !name.is_empty()).count());
        let mut namespace = Self::default();
        for &(at, mnt) in &mounts {
            let Some((root_at, root)) = outermost_holder_of(at, mounts.iter().copied()) else {
                continue;
            };
            let path = at[root_at.len()..].trim_start_matches('/');
            let walker = cred.with_mnt_ns(Arc::new(namespace.clone()));
            let root = Path::root(&namespace.topmost_mount_of(root));
            let spot = namespace
                .where_to_mount(&walker, &root, path)
                .map_err(|errno| UnplacedMount {
                    at: at.to_string(),
                    errno,
                })?;
            namespace.mount(spot, mnt);
        }
        Ok(namespace)
    }

    /// Stand `mnt` on `spot`, marking its dentry a mount point.
    fn mount(&mut self, (parent, mountpoint): (Arc<VfsMount>, Arc<Dentry>), mnt: &Arc<VfsMount>) {
        mountpoint.d_set_mounted();
        self.mounted.push(Arc::new(Mounted {
            parent,
            mountpoint,
            mnt: Arc::clone(mnt),
        }));
    }

    /// Where a mount at `path` beneath `root` stands as `cred` walks it, as
    /// `do_mount` looks its target up and `where_to_mount` stacks it on what is
    /// mounted there: the mount and dentry the walk lands on, or, on a nested
    /// mount's root, the dentry that mount stands on, where the later of the
    /// two is the one a walk steps onto, as Linux shows the topmost. A name
    /// standing for nothing is stood on as it is. A walk that fails answers
    /// its error, and one landing on no directory ENOTDIR, as `graft_tree`
    /// refuses it.
    fn where_to_mount(
        &self,
        cred: &Arc<Cred>,
        root: &Path,
        path: &str,
    ) -> Result<(Arc<VfsMount>, Arc<Dentry>), Errno> {
        let nd = match filename_lookup(cred, root, path, true) {
            Ok(nd) => nd,
            Err(errno) => {
                let nd = filename_parentat(cred, root, path)?;
                let dentry = lookup_dentry(nd.mnt.sb(), nd.dentry(), &nd.last)?;
                return match dentry.inode() {
                    None => Ok((nd.mnt.clone(), dentry)),
                    Some(_) => Err(errno),
                };
            }
        };
        let dentry = nd.path()?;
        if dentry.file_type() != Ok(FileType::Dir) {
            return Err(Errno::NotADirectory);
        }
        if !Arc::ptr_eq(&dentry, nd.mnt.root()) {
            return Ok((nd.mnt.clone(), dentry));
        }
        Ok(
            match self
                .mounted
                .iter()
                .find(|child| Arc::ptr_eq(&child.mnt, &nd.mnt))
            {
                Some(child) => (Arc::clone(&child.parent), Arc::clone(&child.mountpoint)),
                None => (nd.mnt.clone(), dentry),
            },
        )
    }

    /// The mount a walk from the root of the outermost `mnt` stands on: the
    /// last made of those standing over that root, or `mnt` itself.
    pub(crate) fn topmost_mount_of(&self, mnt: &Arc<VfsMount>) -> Arc<VfsMount> {
        self.__lookup_mnt(mnt, mnt.root())
            .unwrap_or_else(|| Arc::clone(mnt))
    }

    /// The mount standing on `dentry` beneath `mnt`, as Linux's `__lookup_mnt`:
    /// the last made of those standing there, which overmounts the rest.
    pub(crate) fn __lookup_mnt(
        &self,
        mnt: &Arc<VfsMount>,
        dentry: &Arc<Dentry>,
    ) -> Option<Arc<VfsMount>> {
        self.mounted
            .iter()
            .rfind(|child| {
                Arc::ptr_eq(&child.parent, mnt) && Arc::ptr_eq(&child.mountpoint, dentry)
            })
            .map(|child| Arc::clone(&child.mnt))
    }

    /// Whether a mount of this namespace stands on `dentry`, as Linux's
    /// `is_local_mountpoint` asks before a removal or a move.
    pub(crate) fn is_local_mountpoint(&self, dentry: &Arc<Dentry>) -> bool {
        dentry.d_mountpoint()
            && self
                .mounted
                .iter()
                .any(|child| Arc::ptr_eq(&child.mountpoint, dentry))
    }

    /// Whether any mount stands beneath `mnt`, as Linux finds its `mnt_mounts`
    /// not empty.
    pub(crate) fn has_children(&self, mnt: &Arc<VfsMount>) -> bool {
        self.mounted
            .iter()
            .any(|child| Arc::ptr_eq(&child.parent, mnt))
    }
}
