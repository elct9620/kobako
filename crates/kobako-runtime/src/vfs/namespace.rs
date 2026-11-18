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
use crate::mount::is_beneath;

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
    /// stands nowhere.
    pub(crate) fn new<'a>(
        cred: &Arc<Cred>,
        mounts: impl IntoIterator<Item = (&'a str, &'a Arc<VfsMount>)>,
    ) -> Self {
        let mut mounts: Vec<_> = mounts.into_iter().collect();
        mounts.sort_by_key(|(at, _)| at.split('/').filter(|name| !name.is_empty()).count());
        let mut namespace = Self::default();
        for &(at, mnt) in &mounts {
            let Some((root_at, root)) = mounts
                .iter()
                .filter(|(above, _)| is_beneath(above, at))
                .min_by_key(|(above, _)| above.len())
            else {
                continue;
            };
            let path = at[root_at.len()..].trim_start_matches('/');
            let walker = cred.with_mnt_ns(Arc::new(namespace.clone()));
            let root = Path::root(&namespace.topmost_mount_of(root));
            if let Some(spot) = namespace.where_to_mount(&walker, &root, path) {
                namespace.mount(spot, mnt);
            }
        }
        namespace
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
    /// `do_mount` looks its target up and `where_to_mount` picks the mount and
    /// dentry: those the walk lands on, or, on a mount's root, where that mount
    /// stands, so the new one stands over it. A name standing for nothing is
    /// stood on as it is. None where the walk fails or lands on no directory.
    fn where_to_mount(
        &self,
        cred: &Arc<Cred>,
        root: &Path,
        path: &str,
    ) -> Option<(Arc<VfsMount>, Arc<Dentry>)> {
        let Ok(nd) = filename_lookup(cred, root, path, true) else {
            let nd = filename_parentat(cred, root, path).ok()?;
            let dentry = lookup_dentry(nd.mnt.sb(), nd.dentry(), &nd.last).ok()?;
            return dentry.inode().is_none().then(|| (nd.mnt.clone(), dentry));
        };
        let dentry = nd.path().ok()?;
        if dentry.file_type() != Ok(FileType::Dir) {
            return None;
        }
        if !Arc::ptr_eq(&dentry, nd.mnt.root()) {
            return Some((nd.mnt.clone(), dentry));
        }
        Some(
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
