//! A name beneath a mount, as Linux's `struct path` pairs a mount with a
//! dentry.

use std::sync::Arc;

use super::dentry::Dentry;
use super::inode::InodeOperations;
use super::mount::VfsMount;
use super::{Error, Metadata};

/// A name beneath a mount, as an open file holds its `f_path`: the mount's
/// root, or an entry opened beneath it. A call relative to one starts at a
/// directory.
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
            dentry: mnt.sb().dentries().root(),
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
        Ok(self.dir().getattr(&self.dentry)?)
    }
}
