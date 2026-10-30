//! Making what a change needs in the upper from what a lower Layer shows,
//! as overlayfs's `copy_up.c` copies a file or a directory up: whole and
//! with the lower one's times before it is named, and leaving the times
//! of the directory it lands in as they were.

use super::super::super::{FileType, FilesystemError, OpenOptions};
use super::super::inode::ovl_inode;
use super::super::path::OverlayPath;
use super::{copy_whole, entry, holds, OverlayFs, ViewDir};
use crate::vfs::{Dentry, RenameFlag};

impl OverlayFs {
    /// The directory `dir` in Layer `into`, made there along with each
    /// directory above it that so far shows only from another Layer, as
    /// overlayfs's `ovl_copy_up` climbs the dentry's parents to copy up
    /// the topmost first. A directory is made in the workdir with the
    /// lower one's times and moved into place, as `ovl_copy_up_workdir`
    /// does.
    pub(super) fn upper_dir(
        &self,
        dir: &ViewDir,
        into: usize,
    ) -> Result<OverlayPath, FilesystemError> {
        if let Some(held) = &dir.level()[into] {
            return Ok(held.clone());
        }
        let Some(above) = dir.dentry.dget_parent() else {
            return Err(FilesystemError::CrossesDevices);
        };
        let parent = self.upper_dir(&self.dir(&above)?, into)?;
        let name = dir.dentry.name();
        let _held = dir.inode.lock.down_write(self.blocking.as_ref())?;
        let made = match entry(&parent, &name)? {
            Some(found) if found.file_type == FileType::Directory => parent.open_dir(&name)?,
            Some(found) if found.file_type == FileType::Symlink => {
                return Err(FilesystemError::CrossesDevices)
            }
            Some(_) => return Err(FilesystemError::NotADirectory),
            None => {
                let lower = dir
                    .level()
                    .iter()
                    .flatten()
                    .next()
                    .ok_or(FilesystemError::NotFound)?
                    .metadata()?;
                let (work, temp) = self.temp()?;
                work.create_dir(&temp)?;
                let made = work.open_dir(&temp).and_then(|made| made.set_times(&lower));
                let parent_times = parent.metadata()?;
                let moved = made.and_then(|()| {
                    work.rename2(&temp, &parent, &name, Some(RenameFlag::NoReplace))
                });
                if let Err(err) = moved {
                    let _ = work.remove_dir(&temp);
                    return Err(err);
                }
                let _ = parent.set_times(&parent_times);
                parent.open_dir(&name)?
            }
        };
        dir.inode.inode_update(into, &made);
        Ok(made)
    }

    /// Copy the file `child` names in `dir` from Layer `from` into Layer
    /// `into`, as overlayfs's `ovl_copy_up_tmpfile` does: the copy is made
    /// with no name, filled, given the lower file's times, and only then
    /// named, so a copy that fails part way is never seen. A link is not
    /// copied up, as no Layer is handed one to hold.
    pub(super) fn copy_up_file(
        &self,
        dir: &ViewDir,
        child: &Dentry,
        from: usize,
        into: usize,
    ) -> Result<(), FilesystemError> {
        let name = child.name();
        let source_dir = dir.level()[from].clone().ok_or(FilesystemError::NotFound)?;
        if entry(&source_dir, &name)?.map(|found| found.file_type) == Some(FileType::Symlink) {
            return Err(FilesystemError::CrossesDevices);
        }
        let target_dir = self.upper_dir(dir, into)?;
        let inode = ovl_inode(child, || Ok(Vec::new()))?;
        let _held = inode.lock.down_write(self.blocking.as_ref())?;
        // Another call may have copied it up while this one waited, as
        // overlayfs's `ovl_already_copied_up_locked` checks.
        if holds(&target_dir, &name)? {
            return Ok(());
        }
        let mut source = source_dir.open(
            &name,
            &OpenOptions {
                read: true,
                ..OpenOptions::default()
            },
        )?;
        let mut copy = target_dir.tmpfile()?;
        copy_whole(source.as_mut(), &mut copy)?;
        let lower = source.getattr()?;
        copy.set_times(lower.accessed, lower.modified);
        let parent_times = target_dir.metadata()?;
        target_dir.link(&copy, &name)?;
        let _ = target_dir.set_times(&parent_times);
        inode.copied_up(&target_dir, &name)
    }
}
