//! Opening, making, removing, and moving a name in the view, as overlayfs's
//! `dir.c` lands each change in the upper layer and whites out what a
//! lower layer still shows. Each change that replaces an entry is built
//! in the workdir and exchanged into place, so it lands whole or not at
//! all.

use super::super::super::{FileOperations, FileType, FilesystemError, OpenOptions};
use super::super::file::reader;
use super::super::path::OverlayPath;
use super::{
    cleanup, cleanup_whiteouts, entry, holds, layer_dir, opaque_inside, Level, OverlayFs, ViewDir,
};
use crate::vfs::{Dentry, RenameFlag};

/// Whether the directory `name` in `dir` holds a whiteout, which
/// overlayfs's `ovl_check_empty_dir` lists for `ovl_clear_empty`.
fn holds_whiteouts(dir: &OverlayPath, name: &str) -> Result<bool, FilesystemError> {
    match dir.open_dir(name) {
        Ok(inner) => Ok(inner
            .entries()?
            .iter()
            .any(|entry| inner.is_whiteout(&entry.name))),
        Err(FilesystemError::NotADirectory) => Ok(false),
        Err(err) => Err(err),
    }
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
                    metadata.file_type != FileType::Directory || opaque_inside(dir, name)?
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

    /// The Layer a whiteout over the Layer at `shown` lands in, or `None`
    /// when nothing below needs one; refused as read-only without an
    /// upper above it.
    fn whiteout_layer(&self, shown: Option<usize>) -> Result<Option<usize>, FilesystemError> {
        match shown {
            Some(shown) => match self.upper()? {
                upper if upper < shown => Ok(Some(upper)),
                _ => Err(FilesystemError::ReadOnlyFilesystem),
            },
            None => Ok(None),
        }
    }

    /// Hide `name` in `dir` with a whiteout made in the workdir first, then
    /// exchanged with what the upper holds there or moved in where it
    /// holds nothing, as overlayfs's `ovl_cleanup_and_whiteout` does.
    fn whiteout(&self, dir: &OverlayPath, name: &str, holds: bool) -> Result<(), FilesystemError> {
        let (work, temp) = self.temp()?;
        work.whiteout(&temp)?;
        let flag = match holds {
            true => RenameFlag::Exchange,
            false => RenameFlag::NoReplace,
        };
        if let Err(err) = work.rename2(&temp, dir, name, Some(flag)) {
            let _ = work.remove_file(&temp);
            return Err(err);
        }
        if holds {
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
        let (work, temp) = self.temp()?;
        let made = make(work, &temp)
            .and_then(|made| {
                work.rename2(&temp, dir, name, Some(RenameFlag::Exchange))?;
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
    /// `ovl_clear_empty` does, so the view shows it empty throughout.
    fn clear_empty(&self, dir: &OverlayPath, name: &str) -> Result<(), FilesystemError> {
        let inner = dir.open_dir(name)?;
        let cleared = inner.entries()?;
        if cleared.iter().any(|entry| !inner.is_whiteout(&entry.name)) {
            return Err(FilesystemError::DirectoryNotEmpty);
        }
        self.exchange_in_opaque_dir(dir, name)
    }

    /// An empty directory marked opaque, exchanged in for what stands at
    /// `name` in `dir`.
    fn exchange_in_opaque_dir(&self, dir: &OverlayPath, name: &str) -> Result<(), FilesystemError> {
        self.exchange_in(dir, name, |work, temp| {
            work.create_dir(temp)?;
            work.open_dir(temp)?.set_opaque()
        })
    }

    /// Take `name` out of the view: a whiteout over any Layer that keeps
    /// it, which also takes the upper's own entry away, or else only the
    /// upper's entry, as overlayfs's `ovl_do_remove` chooses.
    fn withdraw(&self, dir: &ViewDir, name: &str) -> Result<(), FilesystemError> {
        let level = dir.level();
        let (holders, shadowed) = self.holders(&level, name)?;
        let lower = holders.iter().copied().find(|&index| !self.is_upper(index));
        let upper_holds = holders.first().is_some_and(|&index| self.is_upper(index));
        let whiteout_into = self.whiteout_layer(lower.into_iter().chain(shadowed).min())?;
        // A directory the upper holds with whiteouts in it is first
        // replaced by an empty one, as overlayfs's `ovl_clear_empty` does,
        // so it removes as an empty directory, and anything another writer
        // made in it meanwhile refuses the removal.
        if upper_holds && holds_whiteouts(layer_dir(&level[0])?, name)? {
            self.clear_empty(layer_dir(&level[0])?, name)?;
        }
        match whiteout_into {
            Some(upper) => {
                let upper_dir = self.upper_dir(dir, upper)?;
                self.whiteout(&upper_dir, name, upper_holds)
            }
            None if upper_holds => cleanup(layer_dir(&level[0])?, name),
            None => Err(FilesystemError::NotFound),
        }
    }

    /// Open the file `child` names in `dir`. A lower Layer's file opened to
    /// read reads the copy once a copy up makes one.
    pub(in super::super) fn open(
        &self,
        dir: &ViewDir,
        child: &Dentry,
        options: &OpenOptions,
    ) -> Result<Box<dyn FileOperations>, FilesystemError> {
        let name = &child.name();
        let changes = options.write || options.truncate;
        let level = dir.level();
        match self.lookup(&level, name)? {
            Some((_, found)) if found.file_type == FileType::Directory => {
                Err(FilesystemError::IsADirectory)
            }
            Some((index, _)) if self.is_upper(index) => {
                layer_dir(&level[index])?.open(name, options)
            }
            Some((index, _)) if !changes => {
                let file = layer_dir(&level[index])?.open(name, options)?;
                reader(file, child)
            }
            Some((index, _)) => {
                let upper = self.upper()?;
                self.copy_up_file(dir, child, index, upper)?;
                layer_dir(&dir.level()[upper])?.open(name, options)
            }
            None => Err(FilesystemError::NotFound),
        }
    }

    /// Make the file `name`, where the view shows nothing, in the upper.
    pub(in super::super) fn create_file(
        &self,
        dir: &ViewDir,
        name: &str,
        options: &OpenOptions,
    ) -> Result<Box<dyn FileOperations>, FilesystemError> {
        let upper_dir = self.upper_dir(dir, self.upper()?)?;
        if !upper_dir.is_whiteout(name) {
            return upper_dir.atomic_open(name, options);
        }
        self.exchange_in(&upper_dir, name, |work, temp| {
            work.atomic_open(temp, options)
        })
    }

    /// A directory made where a whiteout hid one is marked opaque, so the
    /// lower Layers' entries never show in it.
    pub(in super::super) fn create_dir(
        &self,
        dir: &ViewDir,
        name: &str,
    ) -> Result<(), FilesystemError> {
        if self.lookup(&dir.level(), name)?.is_some() {
            return Err(FilesystemError::AlreadyExists);
        }
        let upper = self.upper()?;
        let upper_dir = self.upper_dir(dir, upper)?;
        match upper_dir.is_whiteout(name) {
            true => self.exchange_in_opaque_dir(&upper_dir, name),
            false => upper_dir.create_dir(name),
        }
    }

    pub(in super::super) fn remove_dir(
        &self,
        dir: &ViewDir,
        name: &str,
    ) -> Result<(), FilesystemError> {
        let level = dir.level();
        let (index, found) = self
            .lookup(&level, name)?
            .ok_or(FilesystemError::NotFound)?;
        if found.file_type != FileType::Directory {
            return Err(FilesystemError::NotADirectory);
        }
        if !self.is_empty(&level, name, (index, found))? {
            return Err(FilesystemError::DirectoryNotEmpty);
        }
        self.withdraw(dir, name)
    }

    /// A link is removed as itself, wherever it points.
    pub(in super::super) fn remove_file(
        &self,
        dir: &ViewDir,
        name: &str,
    ) -> Result<(), FilesystemError> {
        if self.symlink_metadata(&dir.level(), name)?.file_type == FileType::Directory {
            return Err(FilesystemError::IsADirectory);
        }
        self.withdraw(dir, name)
    }

    /// A directory moves only when the upper alone holds it; moving one
    /// out of a lower Layer would mean copying up everything beneath it,
    /// so it is refused as crossing devices, as overlayfs refuses it
    /// without `redirect_dir`; so is a link or another kind a lower Layer
    /// holds, which no copy up can make. What the view holds at `to` is
    /// checked as rename(2) checks it. A directory moved over one a lower
    /// Layer holds is marked opaque before it moves, and the source leaves
    /// a whiteout as it moves when a lower Layer still shows it, as
    /// overlayfs's `ovl_rename` does.
    pub(in super::super) fn rename(
        &self,
        from: &ViewDir,
        moved: &Dentry,
        to: &ViewDir,
        to_name: &str,
    ) -> Result<(), FilesystemError> {
        let from_name = &moved.name();
        let from_level = from.level();
        let moving = self.symlink_metadata(&from_level, from_name)?;
        let to_level = to.level();
        let replaced = self.lookup(&to_level, to_name)?;
        let is_dir = moving.file_type == FileType::Directory;
        // Only a directory replaces a directory, and only an empty one.
        match (is_dir, replaced) {
            (false, Some((_, found))) if found.file_type == FileType::Directory => {
                return Err(FilesystemError::IsADirectory)
            }
            (true, Some((index, found)))
                if found.file_type == FileType::Directory
                    && !self.is_empty(&to_level, to_name, (index, found))? =>
            {
                return Err(FilesystemError::DirectoryNotEmpty)
            }
            (true, Some((_, found))) if found.file_type != FileType::Directory => {
                return Err(FilesystemError::NotADirectory)
            }
            _ => {}
        }
        let upper = self.upper()?;
        let mut replaces_below = false;
        if is_dir {
            for (index, dir) in to_level.iter().enumerate() {
                if let Some(dir) = dir {
                    replaces_below |= index != upper && holds(dir, to_name)?;
                }
            }
        }
        let (holders, shadowed) = self.holders(&from_level, from_name)?;
        let lower = holders.iter().copied().find(|&index| index != upper);
        let shown_below = self
            .whiteout_layer(lower.into_iter().chain(shadowed).min())?
            .is_some();
        match moving.file_type {
            FileType::Directory if holders != [upper] => {
                return Err(FilesystemError::CrossesDevices)
            }
            FileType::File if holders.first() != Some(&upper) => {
                let shown = *holders.first().ok_or(FilesystemError::NotFound)?;
                self.copy_up_file(from, moved, shown, upper)?
            }
            FileType::Directory | FileType::File => {}
            _ if holders.first() != Some(&upper) => return Err(FilesystemError::CrossesDevices),
            _ => {}
        }
        let target_dir = self.upper_dir(to, upper)?;
        let replaces_dir =
            matches!(replaced, Some((_, found)) if found.file_type == FileType::Directory);
        if replaces_dir && holds(&target_dir, to_name)? {
            self.clear_empty(&target_dir, to_name)?;
        }
        let source_dir = from.level()[upper]
            .clone()
            .ok_or(FilesystemError::NotFound)?;
        let moving_dir = match is_dir {
            true => Some(source_dir.open_dir(from_name)?),
            false => None,
        };
        let marked = match &moving_dir {
            Some(dir) if replaces_below && !dir.is_opaque() => {
                dir.set_opaque()?;
                Some(dir)
            }
            _ => None,
        };
        let onto_whiteout = target_dir.is_whiteout(to_name);
        let flag = match (shown_below, onto_whiteout) {
            (true, true) => Some(RenameFlag::Exchange),
            (true, false) => Some(RenameFlag::Whiteout),
            (false, true) if is_dir => Some(RenameFlag::Exchange),
            _ => None,
        };
        if let Err(err) = source_dir.rename2(from_name, &target_dir, to_name, flag) {
            if let Some(dir) = marked {
                let _ = dir.remove_opaque();
            }
            return Err(err);
        }
        if flag == Some(RenameFlag::Exchange) && !shown_below {
            source_dir.remove_file(from_name)?;
        }
        Ok(())
    }
}
