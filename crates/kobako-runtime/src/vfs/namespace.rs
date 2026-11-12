//! Which mount stands on which name, as Linux's mount namespace holds the
//! tree `__lookup_mnt` answers from, so a walk reaching a mount point by any
//! spelling steps onto what is mounted there.

use std::sync::Arc;

use super::dentry::Dentry;
use super::mount::VfsMount;

/// One mount standing on a name beneath another, as Linux's `struct mount`
/// keeps `mnt_parent` and `mnt_mountpoint`. The mount point is kept as names
/// rather than a dentry: a mount stands even where its parent holds nothing by
/// that name, which Linux never lets a mount do.
struct Mounted {
    parent: Arc<VfsMount>,
    /// The names from the parent's root to the directory holding the mount point.
    dir: Vec<String>,
    name: String,
    mnt: Arc<VfsMount>,
}

/// The mounts one Invocation reaches, each under the one whose path holds its
/// own nearest, as Linux's `mnt_namespace` links each mount to its parent.
#[derive(Default)]
pub(crate) struct MntNamespace {
    mounted: Vec<Mounted>,
}

impl MntNamespace {
    /// The tree the mounts at their absolute guest paths make: each one beneath
    /// the mount whose path holds it most closely, none above one no other holds.
    pub(crate) fn new<'a>(mounts: impl IntoIterator<Item = (&'a str, &'a Arc<VfsMount>)>) -> Self {
        let mounts: Vec<_> = mounts.into_iter().collect();
        let mounted = mounts
            .iter()
            .filter_map(|&(at, mnt)| {
                let (parent_at, parent) = mounts
                    .iter()
                    .filter(|(above, _)| beneath(above, at))
                    .max_by_key(|(above, _)| above.len())?;
                let mut names: Vec<String> = at[parent_at.len()..]
                    .split('/')
                    .filter(|name| !name.is_empty())
                    .map(str::to_string)
                    .collect();
                let name = names.pop()?;
                Some(Mounted {
                    parent: Arc::clone(parent),
                    dir: names,
                    name,
                    mnt: Arc::clone(mnt),
                })
            })
            .collect();
        Self { mounted }
    }

    /// Each name in `dir` beneath `mnt` a mount stands on, with what stands
    /// there, as `__lookup_mnt` is asked for each child of a mount.
    pub(crate) fn mounted_in(
        &self,
        mnt: &Arc<VfsMount>,
        dir: &Dentry,
    ) -> Vec<(&str, &Arc<VfsMount>)> {
        let mut children = self
            .mounted
            .iter()
            .filter(|child| Arc::ptr_eq(&child.parent, mnt))
            .peekable();
        if children.peek().is_none() {
            return Vec::new();
        }
        let names = dir.names();
        children
            .filter(|child| child.dir == names)
            .map(|child| (child.name.as_str(), &child.mnt))
            .collect()
    }
}

/// Whether the guest path `inner` lies strictly beneath `outer`.
fn beneath(outer: &str, inner: &str) -> bool {
    outer != inner
        && inner
            .strip_prefix(outer)
            .is_some_and(|rest| outer == "/" || rest.starts_with('/'))
}
