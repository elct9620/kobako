//! Making what a change needs in the upper from what a lower Layer shows,
//! as overlayfs's `copy_up.c` copies a file or a directory up: whole and
//! with the lower one's times before it is named, and leaving the times
//! of the directory it lands in as they were.

use std::sync::Arc;

use super::super::super::{FileType, FilesystemError, OpenOptions};
use super::super::inode::ovl_inode;
use super::super::path::OverlayPath;
use super::{copy_whole, entry, holds, OverlayFs, ViewDir};
use crate::vfs::{Dentry, RenameFlag};

impl OverlayFs {
    /// The directory `dir` in Layer `into`, made there along with each
    /// directory above it that so far shows only from another Layer, as
    /// overlayfs's `ovl_copy_up_flags` climbs the dentry's parents in a loop
    /// to copy up the topmost first, however deep the Layer's tree is.
    pub(super) fn upper_dir(
        &self,
        dir: &ViewDir,
        into: usize,
    ) -> Result<OverlayPath, FilesystemError> {
        let mut unmade = Vec::new();
        let mut at = dir.clone();
        let mut made = loop {
            if let Some(held) = at.level()[into].clone() {
                break held;
            }
            let above = at.dentry.parent().ok_or(FilesystemError::CrossesDevices)?;
            let next = self.dir(&above)?;
            unmade.push(at);
            at = next;
        };
        while let Some(dir) = unmade.pop() {
            made = self.copy_up_dir(&dir, &made, into)?;
        }
        Ok(made)
    }

    /// The directory `dir` in Layer `into`, beneath `parent` there. It is made
    /// in the workdir with the lower one's times and moved into place, as
    /// `ovl_copy_up_workdir` does.
    fn copy_up_dir(
        &self,
        dir: &ViewDir,
        parent: &OverlayPath,
        into: usize,
    ) -> Result<OverlayPath, FilesystemError> {
        let name = dir.dentry.name();
        let _held = dir.inode.lock.down_write()?;
        let made = match entry(parent, &name)? {
            Some(found) if found.file_type == FileType::Directory => parent.open_dir(&name)?,
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
                let moved = made
                    .and_then(|()| work.rename2(&temp, parent, &name, Some(RenameFlag::NoReplace)));
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

    /// Whether `real`, a Layer's dentry, stands in the upper Tmpfs, in its
    /// workdir too, where a removal leaves a file something still holds.
    pub(in super::super) fn is_in_upper(&self, real: &Arc<Dentry>) -> bool {
        let root = self.work.as_ref().and_then(|work| work.dentry().parent());
        root.is_some_and(|root| real.is_subdir(&root))
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
        let target_dir = self.upper_dir(dir, self.upper()?)?;
        let inode = ovl_inode(child, || Ok(Vec::new()))?;
        let _held = inode.lock.down_write()?;
        let real = inode.real().ok_or(FilesystemError::NotFound)?;
        // Another call may have copied it up while this one waited, as
        // overlayfs's `ovl_already_copied_up_locked` checks.
        if self.is_in_upper(&real) {
            return Ok(());
        }
        if holds(&target_dir, &name)? || target_dir.is_whiteout(&name) {
            return Err(FilesystemError::ReadOnlyFilesystem);
        }
        let lower = real.getattr()?;
        if lower.file_type != FileType::File {
            return Err(FilesystemError::PermissionDenied);
        }
        let mut copy = target_dir.tmpfile()?;
        if !truncate {
            let mut source = real.open(&OpenOptions {
                read: true,
                ..OpenOptions::default()
            })?;
            copy_whole(source.as_mut(), &mut copy)?;
        }
        copy.set_times(lower.accessed, lower.modified);
        let parent_times = target_dir.metadata()?;
        target_dir.link(&copy, &name)?;
        let _ = target_dir.set_times(&parent_times);
        inode.set_real(target_dir.real(&name)?);
        Ok(())
    }
}
