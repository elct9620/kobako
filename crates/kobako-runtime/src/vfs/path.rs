//! A name beneath a mount, as Linux's `struct path` pairs a mount with a
//! dentry.

use std::sync::Arc;

use super::dentry::Dentry;
use super::mount::VfsMount;
use super::Errno;

/// Where a walk starts, found once the call has taken its name, as Linux's
/// `path_init` takes the directory a descriptor names only after the name. A
/// caller holding a table of descriptors hands how to find it, so a call with
/// two names answers for the first before it looks at the second's start.
pub trait Start {
    fn path_init(&self) -> Result<Path, Errno>;
}

impl Start for Path {
    fn path_init(&self) -> Result<Path, Errno> {
        Ok(self.clone())
    }
}

impl<F: Fn() -> Result<Path, Errno>> Start for F {
    fn path_init(&self) -> Result<Path, Errno> {
        self()
    }
}

/// A name beneath a mount, as an open file holds its `f_path`: the mount's
/// root, or an entry opened beneath it. A walk starts only from a directory.
#[derive(Clone)]
pub struct Path {
    mnt: Arc<VfsMount>,
    /// The entry, wherever it moved, and whether it was removed.
    dentry: Arc<Dentry>,
}

impl Path {
    /// The root of `mnt`.
    pub fn root(mnt: &Arc<VfsMount>) -> Self {
        Self {
            dentry: mnt.root().clone(),
            mnt: mnt.clone(),
        }
    }

    /// This Path standing at `dentry`.
    pub(crate) fn with_dentry(&self, dentry: Arc<Dentry>) -> Self {
        Self {
            mnt: self.mnt.clone(),
            dentry,
        }
    }

    /// What the entry is mounted on.
    pub fn mnt(&self) -> &Arc<VfsMount> {
        &self.mnt
    }

    pub(crate) fn dentry(&self) -> &Arc<Dentry> {
        &self.dentry
    }
}
