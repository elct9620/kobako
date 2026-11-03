//! Opening, making, removing, and moving a name in the view, as overlayfs's
//! `dir.c` lands each change in the upper layer and whites out what a
//! lower layer still shows. Each change that replaces an entry is built
//! in the workdir and exchanged into place, so it lands whole or not at
//! all.

use super::super::super::{FileOperations, FileType, FilesystemError, OpenHow};
use super::super::file::ovl_file;
use super::super::inode::{OverlayDirInode, OverlayFileInode};
use super::super::path::OverlayPath;
use super::{
    entry, layer_dir, ovl_cleanup, ovl_cleanup_whiteouts, ovl_lower_positive, OverlayFs, ViewDir,
    UPPER,
};
use crate::vfs::{Dentry, RenameFlag};

impl OverlayFs {
    /// Hide `name` in `dir` with a whiteout made in the workdir first, then
    /// moved over what the upper holds there as this call finds it, so a copy up
    /// landing meanwhile is hidden too, as overlayfs's `ovl_cleanup_and_whiteout`
    /// does: only a directory, which no rename replaces while it holds anything,
    /// is exchanged out to be cleaned up.
    fn cleanup_and_whiteout(&self, dir: &OverlayPath, name: &str) -> Result<(), FilesystemError> {
        let (work, temp) = self.tempname()?;
        work.do_whiteout(&temp)?;
        let is_dir = entry(dir, name)?.is_directory();
        let flag = is_dir.then_some(RenameFlag::Exchange);
        if let Err(err) = work.do_rename(&temp, dir, name, flag) {
            let _ = work.do_unlink(&temp);
            return Err(err);
        }
        if is_dir {
            let _ = ovl_cleanup(work, &temp);
        }
        Ok(())
    }

    /// What `make` builds at a name in the workdir, exchanged with what
    /// stands at `name` in `dir`, as overlayfs's `ovl_create_over_whiteout`
    /// and `ovl_clear_empty` exchange theirs. What came out goes with its
    /// whiteouts, and stays in the workdir if another writer put anything
    /// else in it, as overlayfs leaves a cleanup it cannot finish.
    fn exchange_in<T>(
        &self,
        dir: &OverlayPath,
        name: &str,
        make: impl FnOnce(&OverlayPath, &str) -> Result<T, FilesystemError>,
    ) -> Result<T, FilesystemError> {
        let (work, temp) = self.tempname()?;
        let made = make(work, &temp)
            .and_then(|made| {
                work.do_rename(&temp, dir, name, Some(RenameFlag::Exchange))?;
                Ok(made)
            })
            .inspect_err(|_| {
                let _ = ovl_cleanup(work, &temp);
            })?;
        let _ = ovl_cleanup_whiteouts(work, &temp).and_then(|()| ovl_cleanup(work, &temp));
        Ok(made)
    }

    /// An empty directory marked opaque, exchanged in for what stands at `name` in
    /// `dir`, as overlayfs's `ovl_clear_empty` does, so the view shows it empty
    /// throughout. A dentry naming a directory there no longer matches, so the next
    /// walk finds the one exchanged in, should what follows fail.
    pub(in super::super) fn clear_empty(
        &self,
        dir: &OverlayPath,
        name: &str,
    ) -> Result<(), FilesystemError> {
        self.exchange_in(dir, name, |work, temp| {
            work.do_mkdir(temp)?;
            work.open_dir(temp)?.set_opaque()
        })
    }

    /// Whether the upper holds what `dentry` names, read from what its entry
    /// keeps, as `ovl_dentry_upper` answers.
    fn upper_holds(&self, dentry: &Dentry) -> bool {
        let Some(inode) = dentry.inode() else {
            return false;
        };
        match OverlayFileInode::of(&inode) {
            Some(kept) => self.is_in_upper(&kept.real()),
            None => OverlayDirInode::of(&inode).is_some_and(|kept| kept.level()[UPPER].is_some()),
        }
    }

    /// Take `name` out of the view: a whiteout over any Layer that keeps
    /// it, which also takes the upper's own entry away, or else only the
    /// upper's entry, as overlayfs's `ovl_do_remove` chooses. A directory
    /// whose upper holds whiteouts, as `upper_whiteouts` says, is first
    /// replaced by an empty one, as overlayfs's `ovl_clear_empty` does, so it
    /// removes as an empty directory.
    fn do_remove(
        &self,
        dir: &ViewDir,
        dentry: &Dentry,
        upper_whiteouts: bool,
    ) -> Result<(), FilesystemError> {
        let name = &dentry.name();
        let level = dir.level();
        let shown_below = ovl_lower_positive(&level, name);
        let upper_holds = self.upper_holds(dentry);
        if upper_whiteouts {
            self.clear_empty(layer_dir(&level[UPPER])?, name)?;
        }
        match (shown_below, upper_holds) {
            (true, _) => self.cleanup_and_whiteout(&self.upper_dir(dir)?, name),
            (false, true) => ovl_cleanup(layer_dir(&level[UPPER])?, name),
            (false, false) => Err(FilesystemError::NotFound),
        }
    }

    /// Open the file `child` names in `dir` on the Layer's dentry its inode
    /// keeps, copied up first for a change, as `ovl_open` does. A lower Layer's
    /// file opened to read reads the upper's once a copy up makes one.
    pub(in super::super) fn open(
        &self,
        dir: &ViewDir,
        child: &Dentry,
        how: &OpenHow,
    ) -> Result<Box<dyn FileOperations>, FilesystemError> {
        let inode = child.inode().ok_or(FilesystemError::NotFound)?;
        let kept = OverlayFileInode::of(&inode).ok_or(FilesystemError::IsADirectory)?;
        let real = kept.real();
        if self.is_in_upper(&real) {
            return real.open(how);
        }
        if !(how.write || how.truncate) {
            let file = real.open(how)?;
            return Ok(ovl_file(file, real, kept));
        }
        self.copy_up_file(dir, child, how.truncate)?;
        kept.real().open(how)
    }

    /// Make the file `name`, where the view shows nothing, in the upper.
    pub(in super::super) fn create(
        &self,
        dir: &ViewDir,
        name: &str,
    ) -> Result<(), FilesystemError> {
        let upper_dir = self.upper_dir(dir)?;
        match upper_dir.is_whiteout(name) {
            true => self.exchange_in(&upper_dir, name, |work, temp| work.do_create(temp)),
            false => upper_dir.do_create(name),
        }
    }

    /// A directory made where a whiteout hid one is marked opaque, so the
    /// lower Layers' entries never show in it.
    pub(in super::super) fn mkdir(&self, dir: &ViewDir, name: &str) -> Result<(), FilesystemError> {
        let upper_dir = self.upper_dir(dir)?;
        match upper_dir.is_whiteout(name) {
            true => self.clear_empty(&upper_dir, name),
            false => upper_dir.do_mkdir(name),
        }
    }

    pub(in super::super) fn rmdir(
        &self,
        dir: &ViewDir,
        dentry: &Dentry,
    ) -> Result<(), FilesystemError> {
        let upper_whiteouts = self
            .check_empty_dir(dentry)?
            .ok_or(FilesystemError::DirectoryNotEmpty)?;
        self.do_remove(dir, dentry, upper_whiteouts)
    }

    /// A link is removed as itself, wherever it points.
    pub(in super::super) fn unlink(
        &self,
        dir: &ViewDir,
        dentry: &Dentry,
    ) -> Result<(), FilesystemError> {
        self.do_remove(dir, dentry, false)
    }

    /// Whether what `moved` names, of kind `moving`, can move at all, as
    /// `ovl_can_move` reads the dentry's entry: a directory only when the upper
    /// alone holds it, a file always, once copied up, and anything else only from
    /// the upper.
    fn can_move(&self, moved: &Dentry, moving: FileType) -> Result<bool, FilesystemError> {
        if moving == FileType::Dir {
            let level = self.dir(moved)?.level();
            let lower = level.iter().skip(UPPER + 1).flatten().next().is_some();
            return Ok(level[UPPER].is_some() && !lower);
        }
        Ok(moving == FileType::RegFile || self.upper_holds(moved))
    }

    /// A directory moves only when the upper alone holds it; moving one
    /// out of a lower Layer would mean copying up everything beneath it,
    /// so it is refused as crossing devices, as overlayfs refuses it
    /// without `redirect_dir`; so is a link or another kind a lower Layer
    /// holds, which no copy up can make. What the view holds at `to` is
    /// checked as rename(2) checks it. A directory moved into one a lower
    /// Layer also shows is marked opaque before it moves, and the source leaves
    /// a whiteout as it moves when a lower Layer still shows it, as
    /// overlayfs's `ovl_rename` does.
    pub(in super::super) fn rename(
        &self,
        from: &ViewDir,
        moved: &Dentry,
        to: &ViewDir,
        target: &Dentry,
    ) -> Result<(), FilesystemError> {
        let (from_name, to_name) = (&moved.name(), &target.name());
        let from_level = from.level();
        let moving = moved.kind()?;
        let to_level = to.level();
        let is_dir = moving == FileType::Dir;
        // What cannot move answers first, as `ovl_rename_start` asks
        // `ovl_can_move` before it looks at what would be replaced.
        if !self.can_move(moved, moving)? {
            return Err(FilesystemError::CrossesDevices);
        }
        // Only an empty directory is replaced.
        let upper_whiteouts = match target.kind() == Ok(FileType::Dir) {
            true => self
                .check_empty_dir(target)?
                .ok_or(FilesystemError::DirectoryNotEmpty)?,
            false => false,
        };
        let shown_below = ovl_lower_positive(&from_level, from_name);
        if moving == FileType::RegFile {
            self.copy_up_file(from, moved, false)?;
        }
        let target_dir = self.upper_dir(to)?;
        // A directory holding only whiteouts is replaced by an empty one first,
        // as `ovl_rename_upper` calls `ovl_clear_empty` for a list not empty.
        if upper_whiteouts {
            self.clear_empty(&target_dir, to_name)?;
        }
        let source_dir = from.level()[UPPER]
            .clone()
            .ok_or(FilesystemError::NotFound)?;
        // Nothing below shows a directory the upper alone holds, so a mark left
        // on one by a move that then fails changes nothing, as `ovl_rename`
        // leaves it.
        let into_merged = to_level
            .iter()
            .enumerate()
            .any(|(index, dir)| index != UPPER && dir.is_some());
        if is_dir && into_merged {
            source_dir.open_dir(from_name)?.set_opaque()?;
        }
        let onto_whiteout = target_dir.is_whiteout(to_name);
        let flag = match (shown_below, onto_whiteout) {
            (true, true) => Some(RenameFlag::Exchange),
            (true, false) => Some(RenameFlag::Whiteout),
            (false, true) if is_dir => Some(RenameFlag::Exchange),
            _ => None,
        };
        source_dir.do_rename(from_name, &target_dir, to_name, flag)?;
        if flag == Some(RenameFlag::Exchange) && !shown_below {
            source_dir.do_unlink(from_name)?;
        }
        Ok(())
    }
}
