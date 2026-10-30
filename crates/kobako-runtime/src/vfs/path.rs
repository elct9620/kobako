//! A name beneath a mount, as Linux's `struct path` pairs a mount with a
//! dentry.

use std::sync::Arc;

use super::dentry::Dentry;
use super::mount::VfsMount;
use super::times::mask_times;
use super::Error;
use crate::filesystem::{InodeOperations, Metadata};
use crate::profile::Profile;

/// A name beneath a mount, as an open file holds its `f_path`: the mount's
/// root, or an entry opened beneath it. A call relative to one starts at a
/// directory.
#[derive(Clone)]
pub struct Path {
    mnt: Arc<VfsMount>,
    /// The entry, wherever it moved, and whether it was removed.
    dentry: Arc<Dentry>,
    /// The isolation the invocation holding it runs under, which decides
    /// whether the times a Filesystem reports reach the guest.
    profile: Profile,
}

impl Path {
    /// The root of `mnt`, for an invocation running under `profile`.
    pub fn root(mnt: &Arc<VfsMount>, profile: Profile) -> Self {
        Self {
            dentry: mnt.filesystem().dentries().root(),
            mnt: mnt.clone(),
            profile,
        }
    }

    /// This Path standing at `dentry`.
    pub(crate) fn with_dentry(&self, dentry: Arc<Dentry>) -> Self {
        Self {
            mnt: self.mnt.clone(),
            dentry,
            profile: self.profile,
        }
    }

    pub(crate) fn profile(&self) -> Profile {
        self.profile
    }

    /// What the entry is mounted on.
    pub fn mnt(&self) -> &Arc<VfsMount> {
        &self.mnt
    }

    pub(crate) fn dentry(&self) -> &Arc<Dentry> {
        &self.dentry
    }

    /// The directory itself, where a call through it starts. A Path is made
    /// only for a dentry recording its directory, which a dentry never
    /// gives up.
    pub(crate) fn dir(&self) -> Arc<dyn InodeOperations> {
        self.dentry
            .inode()
            .expect("a Path's dentry records its directory")
            .ops()
            .clone()
    }

    /// What the directory reports of itself.
    pub fn metadata(&self) -> Result<Metadata, Error> {
        Ok(mask_times(self.dir().getattr(&self.dentry)?, self.profile))
    }
}
