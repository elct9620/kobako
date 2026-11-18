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

/// The mounts one Invocation reaches, each standing on a dentry of the mount
/// its path's walk landed in, as Linux's `mnt_namespace` links each mount to
/// its parent.
#[derive(Default)]
pub(crate) struct MntNamespace {
    mounted: Vec<Arc<Mounted>>,
}

impl MntNamespace {
    /// The tree the mounts at their absolute guest paths make, each placed as
    /// Linux's `mount` places it: its path walked through the mounts placed
    /// before it, from the root of the outermost mount holding that path,
    /// following a link at its last name too, as `do_mount` looks its target
    /// up with `LOOKUP_FOLLOW`, never above that root. Shallower paths go
    /// first, as a container runtime mounts a parent before its children, and
    /// of two landing where one stands, the later stands over the earlier. A
    /// link leading out of that root, or to nothing, leaves the mount on the
    /// name its path spells. The last name may stand for nothing, which Linux
    /// never lets a mount do, so the parent's name is hidden even once made. A
    /// mount whose directories above it stand nowhere is reached by no walk.
    pub(crate) fn new<'a>(
        cred: &Arc<Cred>,
        mounts: impl IntoIterator<Item = (&'a str, &'a Arc<VfsMount>)>,
    ) -> Self {
        let mut mounts: Vec<_> = mounts.into_iter().collect();
        mounts.sort_by_key(|(at, _)| at.split('/').filter(|name| !name.is_empty()).count());
        let mut placed = Self::default();
        for &(at, mnt) in &mounts {
            let Some((root_at, root)) = mounts
                .iter()
                .filter(|(above, _)| beneath(above, at))
                .min_by_key(|(above, _)| above.len())
            else {
                continue;
            };
            let walker = cred.with_mnt_ns(Arc::new(Self {
                mounted: placed.mounted.clone(),
            }));
            let Some((parent, mountpoint)) =
                placed.get_mountpoint(&walker, root, &at[root_at.len()..])
            else {
                continue;
            };
            mountpoint.d_set_mounted();
            placed.mounted.push(Arc::new(Mounted {
                parent,
                mountpoint,
                mnt: Arc::clone(mnt),
            }));
        }
        placed
    }

    /// Where a mount at `path` beneath the root of `root` stands, as `cred`
    /// walks it: the mount and the dentry, or where a mount the walk landed on
    /// stands, so the new one stands over it. A walk that fails at the link it
    /// follows, or lands on a root no mount of this namespace stands on, such
    /// as the outermost mount's, leaves the mount on the name `path` spells, so
    /// a preopen and a walk still meet it there; none stands where a directory
    /// above that name stands nowhere.
    fn get_mountpoint(
        &self,
        cred: &Arc<Cred>,
        root: &Arc<VfsMount>,
        path: &str,
    ) -> Option<(Arc<VfsMount>, Arc<Dentry>)> {
        let path = path.trim_start_matches('/');
        let root = Path::root(root);
        if let Ok(nd) = filename_lookup(cred, &root, path, true) {
            let dentry = nd.path().ok()?;
            if !Arc::ptr_eq(&dentry, nd.mnt.root()) {
                return Some((nd.mnt.clone(), dentry));
            }
            if let Some(child) = self
                .mounted
                .iter()
                .rfind(|child| Arc::ptr_eq(&child.mnt, &nd.mnt))
            {
                return Some((Arc::clone(&child.parent), Arc::clone(&child.mountpoint)));
            }
        }
        let nd = filename_parentat(cred, &root, path).ok()?;
        let dentry = lookup_dentry(nd.mnt.sb(), nd.dentry(), &nd.last).ok()?;
        Some((nd.mnt.clone(), dentry))
    }

    /// The mount a walk to where `mnt` stands steps onto: the last made of
    /// those standing there, or `mnt` itself where none stands over it.
    pub(crate) fn topmost_of(&self, mnt: &Arc<VfsMount>) -> Arc<VfsMount> {
        self.mounted
            .iter()
            .find(|child| Arc::ptr_eq(&child.mnt, mnt))
            .and_then(|child| self.__lookup_mnt(&child.parent, &child.mountpoint))
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

/// Whether the guest path `inner` lies strictly beneath `outer`.
fn beneath(outer: &str, inner: &str) -> bool {
    outer != inner
        && inner
            .strip_prefix(outer)
            .is_some_and(|rest| outer == "/" || rest.starts_with('/'))
}
