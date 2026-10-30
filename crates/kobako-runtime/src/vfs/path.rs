//! A directory beneath a Mount, as Linux's `struct path` pairs a mount
//! with a dentry.

use std::sync::Arc;

use super::dentry::Dentry;
use super::namei::down_read;
use super::times::mask_times;
use super::Error;
use crate::filesystem::{DirEntry, FileType, FilesystemError, InodeOperations, Lookup, Metadata};
use crate::mount::Mount;
use crate::profile::Profile;

/// A directory beneath a Mount, as a directory descriptor holds one: the
/// Mount's root, or a directory opened beneath it.
#[derive(Clone)]
pub struct Path {
    mount: Arc<Mount>,
    /// The directory, wherever it moved, and whether it was removed.
    dentry: Arc<Dentry>,
    /// The isolation the invocation holding it runs under, which decides
    /// whether the times a Filesystem reports reach the guest.
    profile: Profile,
}

impl Path {
    /// The root of `mount`, for an invocation running under `profile`.
    pub fn root(mount: Mount, profile: Profile) -> Self {
        Self {
            dentry: mount.filesystem().dentries().root(),
            mount: Arc::new(mount),
            profile,
        }
    }

    /// This Path standing in the directory `dentry` records.
    pub(crate) fn with_directory(&self, dentry: Arc<Dentry>) -> Self {
        Self {
            mount: self.mount.clone(),
            dentry,
            profile: self.profile,
        }
    }

    pub(crate) fn profile(&self) -> Profile {
        self.profile
    }

    /// The Mount the directory stands beneath.
    pub fn mount(&self) -> &Arc<Mount> {
        &self.mount
    }

    pub(crate) fn dentry(&self) -> &Arc<Dentry> {
        &self.dentry
    }

    /// The directory itself, where a call through it starts. A Path is made
    /// only for a dentry recording its directory, which a dentry never
    /// gives up.
    pub(crate) fn dir(&self) -> Arc<dyn InodeOperations> {
        self.dentry
            .d_inode()
            .expect("a Path's dentry records its directory")
            .ops()
            .clone()
    }

    /// Refuse a call through the directory, beneath a Mount whose Policy
    /// judges paths, unless its dentry still names what the runtime opened
    /// there, so the Policy judges the path the directory stands at. A
    /// dentry dropped or removed is refused as Linux's AppArmor refuses an
    /// unlinked one; one held is asked of the directory above through
    /// `d_revalidate`, walking there from the Mount's root as gVisor's
    /// `revalidatePath` walks; and one whose identity the runtime never
    /// learned cannot be told from another, so it is refused too. Linux
    /// asks nothing of the dentry a walk starts from; kobako adds this so a
    /// host or another Filesystem instance moving a directory behind the
    /// runtime's back never has a Policy judge a path it no longer stands at.
    pub(crate) fn revalidate_path(&self) -> Result<(), Error> {
        let Some(parent) = self.dentry.dget_parent() else {
            return Ok(());
        };
        if !self.mount.judges_paths() {
            return Ok(());
        }
        let filesystem = self.mount.filesystem();
        let dentries = filesystem.dentries();
        let told_apart = self
            .dentry
            .d_inode()
            .is_some_and(|inode| inode.answers_by_path() || inode.identity().is_some());
        if dentries.d_unlinked(&self.dentry) || !told_apart {
            return Err(FilesystemError::NotFound.into());
        }
        // No `d_revalidate` means a name stays valid, as Linux's d_op does.
        if filesystem.root().d_revalidate(&self.dentry).is_none() {
            return Ok(());
        }
        let mut above = Vec::new();
        let mut at = Some(parent);
        while let Some(next) = at {
            at = next.dget_parent();
            above.push(next);
        }
        let mut dir = filesystem.root().clone();
        for ancestor in above.iter().rev().skip(1) {
            dir = match dir.lookup(ancestor) {
                Some(Ok(Lookup::Found(inode)))
                    if matches!(inode.kind(), Some(FileType::Directory) | None) =>
                {
                    inode.ops().clone()
                }
                Some(Err(err))
                    if !matches!(
                        err,
                        FilesystemError::NotFound | FilesystemError::NotADirectory
                    ) =>
                {
                    return Err(err.into())
                }
                _ => return Err(FilesystemError::NotFound.into()),
            };
        }
        if dir.d_revalidate(&self.dentry).unwrap_or(Ok(true))? {
            return Ok(());
        }
        dentries.d_invalidate(&self.dentry);
        Err(FilesystemError::NotFound.into())
    }

    /// What the directory reports of itself.
    pub fn metadata(&self) -> Result<Metadata, Error> {
        self.revalidate_path()?;
        Ok(mask_times(self.dir().getattr(&self.dentry)?, self.profile))
    }

    /// The entries of the directory, without `.` or `..`; none once it was
    /// removed, as Linux lists a dead directory.
    pub fn entries(&self) -> Result<Vec<DirEntry>, Error> {
        self.revalidate_path()?;
        // A listing holds the directory shared, as Linux's `iterate_dir`
        // does, so a removal through the VFS lands before or after it.
        let _held = down_read(&self.mount, &self.dentry)?;
        if self.dentry.is_dead() {
            return Ok(Vec::new());
        }
        Ok(self
            .dir()
            .entries(&self.dentry)
            .unwrap_or(Err(FilesystemError::NotADirectory))?)
    }
}
