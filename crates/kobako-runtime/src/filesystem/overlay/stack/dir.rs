//! Opening, making, removing, and moving a name in the view, as overlayfs's
//! `dir.c` lands each change in the upper layer and whites out what a
//! lower layer still shows. Each change that replaces an entry is built
//! in the workdir and exchanged into place, so it lands whole or not at
//! all.

use super::super::super::{FileOperations, FileType, FilesystemError, OpenOptions};
use super::super::file::ovl_file;
use super::super::inode::ovl_inode_of;
use super::super::path::OverlayPath;
use super::{
    cleanup, cleanup_whiteouts, entry, holds, layer_dir, Level, OverlayFs, ViewDir, UPPER,
};
use crate::vfs::{Dentry, RenameFlag};

/// Whether the directory `name` in `dir` holds a whiteout, which
/// overlayfs's `ovl_check_empty_dir` lists for `ovl_clear_empty`.
fn holds_whiteouts(dir: &OverlayPath, name: &str) -> Result<bool, FilesystemError> {
    let inner = dir.open_dir(name)?;
    Ok(inner
        .entries()?
        .iter()
        .any(|entry| inner.is_whiteout(&entry.name)))
}

impl OverlayFs {
    /// Every Layer still showing `name` in `level`, topmost first, and the
    /// topmost Layer below them whose entry there a file or link above
    /// shadows.
    fn holders(
        &self,
        level: &Level,
        name: &str,
    ) -> Result<(Vec<usize>, Option<usize>), FilesystemError> {
        let mut holders = Vec::new();
        for (index, dir) in level.iter().enumerate() {
            let Some(dir) = dir else {
                continue;
            };
            let found = entry(dir, name)?;
            let hidden = match &found {
                Some(metadata) => {
                    holders.push(index);
                    metadata.file_type != FileType::Directory || dir.open_dir(name)?.is_opaque()
                }
                None => dir.is_whiteout(name),
            };
            if hidden {
                let shadowed = match found {
                    Some(_) => self.beneath(level, index, name)?,
                    None => None,
                };
                return Ok((holders, shadowed));
            }
        }
        Ok((holders, None))
    }

    /// The topmost Layer below `above` holding `name` in sight.
    fn beneath(
        &self,
        level: &Level,
        above: usize,
        name: &str,
    ) -> Result<Option<usize>, FilesystemError> {
        for (index, dir) in level.iter().enumerate().skip(above + 1) {
            let Some(dir) = dir else {
                continue;
            };
            if holds(dir, name)? {
                return Ok(Some(index));
            }
            if dir.is_whiteout(name) {
                return Ok(None);
            }
        }
        Ok(None)
    }

    /// Hide `name` in `dir` with a whiteout made in the workdir first, then
    /// moved over what the upper holds there as this call finds it, so a copy up
    /// landing meanwhile is hidden too, as overlayfs's `ovl_cleanup_and_whiteout`
    /// does: only a directory, which no rename replaces while it holds anything,
    /// is exchanged out to be cleaned up.
    fn cleanup_and_whiteout(&self, dir: &OverlayPath, name: &str) -> Result<(), FilesystemError> {
        let (work, temp) = self.tempname()?;
        work.do_whiteout(&temp)?;
        let is_dir = entry(dir, name)?.is_some_and(|found| found.file_type == FileType::Directory);
        let flag = is_dir.then_some(RenameFlag::Exchange);
        if let Err(err) = work.do_rename(&temp, dir, name, flag) {
            let _ = work.do_unlink(&temp);
            return Err(err);
        }
        if is_dir {
            let _ = cleanup(work, &temp);
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
                let _ = cleanup(work, &temp);
            })?;
        let _ = cleanup_whiteouts(work, &temp).and_then(|()| cleanup(work, &temp));
        Ok(made)
    }

    /// Replace the upper's directory `name` in `dir`, holding nothing but
    /// whiteouts, with an empty one marked opaque, as overlayfs's
    /// `ovl_clear_empty` does, so the view shows it empty throughout. Its
    /// dentry in the view no longer matches, so the next walk finds the one
    /// exchanged in, should what follows fail.
    pub(in super::super) fn clear_empty(
        &self,
        dir: &OverlayPath,
        name: &str,
    ) -> Result<(), FilesystemError> {
        self.exchange_in_opaque_dir(dir, name)
    }

    /// An empty directory marked opaque, exchanged in for what stands at
    /// `name` in `dir`.
    fn exchange_in_opaque_dir(&self, dir: &OverlayPath, name: &str) -> Result<(), FilesystemError> {
        self.exchange_in(dir, name, |work, temp| {
            work.do_mkdir(temp)?;
            work.open_dir(temp)?.set_opaque()
        })
    }

    /// Take `name` out of the view: a whiteout over any Layer that keeps
    /// it, which also takes the upper's own entry away, or else only the
    /// upper's entry, as overlayfs's `ovl_do_remove` chooses.
    fn do_remove(&self, dir: &ViewDir, name: &str, is_dir: bool) -> Result<(), FilesystemError> {
        let level = dir.level();
        let (holders, shadowed) = self.holders(&level, name)?;
        let shown_below = holders.iter().any(|&index| index != UPPER) || shadowed.is_some();
        let upper_holds = holders.first() == Some(&UPPER);
        // A directory the upper holds with whiteouts in it is first
        // replaced by an empty one, as overlayfs's `ovl_clear_empty` does,
        // so it removes as an empty directory.
        if is_dir && upper_holds && holds_whiteouts(layer_dir(&level[UPPER])?, name)? {
            self.clear_empty(layer_dir(&level[UPPER])?, name)?;
        }
        match (shown_below, upper_holds) {
            (true, _) => self.cleanup_and_whiteout(&self.upper_dir(dir)?, name),
            (false, true) => cleanup(layer_dir(&level[UPPER])?, name),
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
        options: &OpenOptions,
    ) -> Result<Box<dyn FileOperations>, FilesystemError> {
        let inode = child.inode().ok_or(FilesystemError::NotFound)?;
        let kept = ovl_inode_of(&inode).ok_or(FilesystemError::IsADirectory)?;
        let real = kept.real().ok_or(FilesystemError::IsADirectory)?;
        if self.is_in_upper(&real) {
            return real.open(options);
        }
        if !(options.write || options.truncate) {
            let file = real.open(options)?;
            return Ok(ovl_file(file, real, kept));
        }
        self.copy_up_file(dir, child, options.truncate)?;
        kept.real()
            .ok_or(FilesystemError::IsADirectory)?
            .open(options)
    }

    /// Make the file `name`, where the view shows nothing, in the upper.
    pub(in super::super) fn create(
        &self,
        dir: &ViewDir,
        name: &str,
        options: &OpenOptions,
    ) -> Result<Box<dyn FileOperations>, FilesystemError> {
        let upper_dir = self.upper_dir(dir)?;
        if !upper_dir.is_whiteout(name) {
            return upper_dir.atomic_open(name, options);
        }
        self.exchange_in(&upper_dir, name, |work, temp| {
            work.atomic_open(temp, options)
        })
    }

    /// A directory made where a whiteout hid one is marked opaque, so the
    /// lower Layers' entries never show in it.
    pub(in super::super) fn mkdir(&self, dir: &ViewDir, name: &str) -> Result<(), FilesystemError> {
        let upper_dir = self.upper_dir(dir)?;
        match upper_dir.is_whiteout(name) {
            true => self.exchange_in_opaque_dir(&upper_dir, name),
            false => upper_dir.do_mkdir(name),
        }
    }

    pub(in super::super) fn rmdir(
        &self,
        dir: &ViewDir,
        dentry: &Dentry,
    ) -> Result<(), FilesystemError> {
        if !self.check_empty_dir(dentry)? {
            return Err(FilesystemError::DirectoryNotEmpty);
        }
        self.do_remove(dir, &dentry.name(), true)
    }

    /// A link is removed as itself, wherever it points.
    pub(in super::super) fn unlink(
        &self,
        dir: &ViewDir,
        name: &str,
    ) -> Result<(), FilesystemError> {
        self.do_remove(dir, name, false)
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
        let is_dir = moving == FileType::Directory;
        let (holders, shadowed) = self.holders(&from_level, from_name)?;
        // What cannot move answers first, as `ovl_rename_start` asks
        // `ovl_can_move` before it looks at what would be replaced.
        let holds_above = holders.first() == Some(&UPPER);
        match moving {
            FileType::Directory if holders != [UPPER] => {
                return Err(FilesystemError::CrossesDevices)
            }
            FileType::Directory | FileType::File => {}
            _ if !holds_above => return Err(FilesystemError::CrossesDevices),
            _ => {}
        }
        // Only an empty directory is replaced.
        let replaces_dir = target.kind() == Ok(FileType::Directory);
        if replaces_dir && !self.check_empty_dir(target)? {
            return Err(FilesystemError::DirectoryNotEmpty);
        }
        let shown_below = holders.iter().any(|&index| index != UPPER) || shadowed.is_some();
        if moving == FileType::File && !holds_above {
            self.copy_up_file(from, moved, false)?;
        }
        let target_dir = self.upper_dir(to)?;
        // A directory holding only whiteouts is replaced by an empty one first,
        // as `ovl_rename_upper` calls `ovl_clear_empty` for a list not empty.
        if replaces_dir && holds(&target_dir, to_name)? && holds_whiteouts(&target_dir, to_name)? {
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
            let moving = source_dir.open_dir(from_name)?;
            if !moving.is_opaque() {
                moving.set_opaque()?;
            }
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
