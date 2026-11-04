//! Making what a change needs in the upper from what a lower Layer shows,
//! as overlayfs's `copy_up.c` copies a file or a directory up: whole and
//! with the lower one's times before it is named, and leaving the times
//! of the directory it lands in as they were.

use std::sync::Arc;

use super::super::super::{FileType, FilesystemError, OpenHow};
use super::super::inode::OverlayFileInode;
use super::super::path::{timestamps, OverlayPath};
use super::{copy_whole, OverlayFs, ViewDir, UPPER};
use crate::vfs::{Dentry, RenameFlag};

impl OverlayFs {
    /// The directory `dir` in the upper, made there along with each directory
    /// above it that so far shows only from another Layer, as overlayfs's
    /// `ovl_copy_up_flags` climbs the dentry's parents in a loop to copy up the
    /// topmost first, however deep the Layer's tree is.
    pub(in super::super) fn upper_dir(
        &self,
        dir: &ViewDir,
    ) -> Result<OverlayPath, FilesystemError> {
        let mut unmade = Vec::new();
        let mut at = dir.clone();
        let mut made = loop {
            if let Some(held) = at.level()[UPPER].clone() {
                break held;
            }
            let next = self.dir(&at.dentry.parent())?;
            unmade.push(at);
            at = next;
        };
        while let Some(dir) = unmade.pop() {
            made = self.copy_up_dir(&dir, &made)?;
        }
        Ok(made)
    }

    /// The directory `dir` in the upper, beneath `parent` there. It is made in
    /// the workdir with the lower one's times and moved into place, as
    /// `ovl_copy_up_workdir` does. Another call may have copied it up while this
    /// one waited, as `ovl_already_copied_up_locked` checks; what else stands at
    /// the name is no copy of this directory, and refuses the move.
    fn copy_up_dir(
        &self,
        dir: &ViewDir,
        parent: &OverlayPath,
    ) -> Result<OverlayPath, FilesystemError> {
        let name = dir.dentry.name();
        let _held = dir.inode.lock.down_write()?;
        let level = dir.level();
        if let Some(held) = &level[UPPER] {
            return Ok(held.clone());
        }
        let lower = level
            .iter()
            .flatten()
            .next()
            .ok_or(FilesystemError::NotFound)?
            .getattr()?;
        let (work, temp) = self.tempname()?;
        work.do_mkdir(&temp)?;
        let made = work
            .open_dir(&temp)
            .and_then(|made| made.set_timestamps(&lower));
        let parent_times = parent.getattr()?;
        let moved =
            made.and_then(|()| work.do_rename(&temp, parent, &name, Some(RenameFlag::NoReplace)));
        if let Err(err) = moved {
            let _ = work.do_rmdir(&temp);
            return Err(err);
        }
        let _ = parent.set_timestamps(&parent_times);
        let made = parent.open_dir(&name)?;
        dir.inode.inode_update(&made);
        Ok(made)
    }

    /// Whether `real`, a Layer's dentry, stands in the upper Tmpfs, in its
    /// workdir too, where a removal leaves a file something still holds.
    pub(in super::super) fn is_in_upper(&self, real: &Arc<Dentry>) -> bool {
        self.work
            .as_ref()
            .is_some_and(|work| real.is_subdir(&work.dentry().parent()))
    }

    /// Copy the file `child` names in `dir` up from the Layer its inode keeps,
    /// as overlayfs's `ovl_copy_up_tmpfile` does: the copy is made with no
    /// name, filled, given the lower file's times, and only then named, so a
    /// copy that fails part way is never seen. A `truncate` copy up leaves the
    /// bytes behind, as `ovl_copy_up_one` sets its size to zero for `O_TRUNC`,
    /// since the open is about to empty it. Only a file copies up, as
    /// overlayfs's `ovl_copy_up_data` only reads a regular file, so a special
    /// file is never opened. A file whose name is gone from the view has no
    /// place to land.
    pub(in super::super) fn copy_up_file(
        &self,
        dir: &ViewDir,
        child: &Dentry,
        truncate: bool,
    ) -> Result<(), FilesystemError> {
        let name = child.name();
        let target_dir = self.upper_dir(dir)?;
        let inode = child
            .inode()
            .as_deref()
            .and_then(OverlayFileInode::of)
            .ok_or(FilesystemError::NotFound)?;
        let _held = inode.lock.down_write()?;
        let real = inode.real();
        // Another call may have copied it up while this one waited, as
        // overlayfs's `ovl_already_copied_up_locked` checks.
        if self.is_in_upper(&real) {
            return Ok(());
        }
        let lower = real.getattr()?;
        if lower.file_type != FileType::RegFile {
            return Err(FilesystemError::PermissionDenied);
        }
        let mut copy = target_dir.do_tmpfile()?;
        if !truncate {
            let mut source = real.open(&OpenHow {
                read: true,
                ..OpenHow::default()
            })?;
            copy_whole(source.as_mut(), &mut copy)?;
        }
        copy.setattr(&timestamps(&lower));
        let parent_times = target_dir.getattr()?;
        let linked = target_dir.do_link(&copy, &name)?;
        let _ = target_dir.set_timestamps(&parent_times);
        inode.set_real(linked);
        Ok(())
    }
}
