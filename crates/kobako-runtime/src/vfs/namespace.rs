//! Which mount stands on which dentry, as Linux's mount namespace holds the
//! tree `__lookup_mnt` answers from, so a walk reaching a mount point by any
//! spelling, through any link, or after a directory above it moved steps onto
//! what is mounted there.

use std::sync::Arc;

use super::dentry::Dentry;
use super::mount::VfsMount;
use super::namei::{filename_parentat, lookup_dentry};
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
        self.mountpoint.put_mountpoint();
    }
}

/// The mounts one Invocation reaches, each under the one whose path holds its
/// own nearest, as Linux's `mnt_namespace` links each mount to its parent.
#[derive(Default)]
pub(crate) struct MntNamespace {
    mounted: Vec<Mounted>,
}

impl MntNamespace {
    /// The tree the mounts at their absolute guest paths make: each one beneath
    /// the mount whose path holds it most closely, none above one no other
    /// holds. Each stands on the dentry its path reaches in that parent as
    /// `cred` walks it, following links, as `do_mount` looks its target up with
    /// `LOOKUP_FOLLOW`. The last name may stand for nothing, which Linux never
    /// lets a mount do, so the parent's name is hidden even once made. A mount
    /// whose directories above it stand nowhere in its parent is reached by no
    /// walk.
    pub(crate) fn new<'a>(
        cred: &Arc<Cred>,
        mounts: impl IntoIterator<Item = (&'a str, &'a Arc<VfsMount>)>,
    ) -> Self {
        let mounts: Vec<_> = mounts.into_iter().collect();
        let mounted = mounts
            .iter()
            .filter_map(|&(at, mnt)| {
                let (parent_at, parent) = mounts
                    .iter()
                    .filter(|(above, _)| beneath(above, at))
                    .max_by_key(|(above, _)| above.len())?;
                let mountpoint = get_mountpoint(cred, parent, &at[parent_at.len()..])?;
                Some(Mounted {
                    parent: Arc::clone(parent),
                    mountpoint,
                    mnt: Arc::clone(mnt),
                })
            })
            .collect();
        Self { mounted }
    }

    /// The mount standing on `dentry` beneath `mnt`, as Linux's `__lookup_mnt`.
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

    /// Whether any mount stands beneath `mnt`, as Linux's `mnt_has_children`.
    pub(crate) fn mnt_has_children(&self, mnt: &Arc<VfsMount>) -> bool {
        self.mounted
            .iter()
            .any(|child| Arc::ptr_eq(&child.parent, mnt))
    }
}

/// The dentry `path` reaches beneath the root of `parent` as `cred` walks it,
/// marked a mount point, as `do_mount` looks its target up and
/// `get_mountpoint` pins it; none where a directory above its last name
/// stands nowhere.
fn get_mountpoint(cred: &Arc<Cred>, parent: &Arc<VfsMount>, path: &str) -> Option<Arc<Dentry>> {
    let path = path.trim_start_matches('/');
    let nd = filename_parentat(cred, &Path::root(parent), path).ok()?;
    let dentry = lookup_dentry(nd.mnt.sb(), nd.dentry(), &nd.last).ok()?;
    dentry.d_set_mounted();
    Some(dentry)
}

/// Whether the guest path `inner` lies strictly beneath `outer`.
fn beneath(outer: &str, inner: &str) -> bool {
    outer != inner
        && inner
            .strip_prefix(outer)
            .is_some_and(|rest| outer == "/" || rest.starts_with('/'))
}
