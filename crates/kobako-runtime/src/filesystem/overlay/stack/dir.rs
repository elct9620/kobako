//! Opening, making, removing, and moving a name in the view, as overlayfs's
//! `dir.c` lands each change in the upper layer and whites out what a
//! lower layer still shows.

use super::super::super::path::{beneath_itself, is_beneath};
use super::super::super::{File, FileType, FilesystemError, OpenOptions, Rename};
use super::super::path::OverlayPath;
use super::{
    entry, hides, holds, layer_dir, opaque_inside, put_marker, refuse_reserved, whiteout,
    without_markers, Chain, Level, OverlayFs, OPAQUE, WHITEOUT,
};

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
                    metadata.file_type != FileType::Directory
                        || hides(dir, name)?
                        || opaque_inside(dir, name)?
                }
                None => hides(dir, name)?,
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
            if hides(dir, name)? {
                return Ok(None);
            }
        }
        Ok(None)
    }

    /// Clear the whiteout of `name` in `dir` once an entry has landed
    /// there. A whiteout left beside the entry hides only what lies below,
    /// which the entry replaced, so failing to clear it fails nothing.
    fn unhide(dir: &OverlayPath, name: &str) {
        let _ = dir.remove_file(&whiteout(name));
    }

    fn mark(&self, chain: &mut Chain, marker: &str, into: usize) -> Result<(), FilesystemError> {
        put_marker(&self.upper_dir(chain, into)?, marker)
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

    /// Take `name` out of the view: a whiteout over any Layer that keeps it
    /// first, then out of the upper. A lower Layer keeps it, and so does
    /// the upper where `keeps` answers for it, so no Layer changes before
    /// each is known to go; `remove` takes it out of one Layer's directory.
    fn withdraw(
        &self,
        chain: &mut Chain,
        name: &str,
        keeps: impl Fn(&OverlayPath) -> Result<bool, FilesystemError>,
        remove: impl Fn(&OverlayPath) -> Result<(), FilesystemError>,
    ) -> Result<(), FilesystemError> {
        let level = chain.level().clone();
        let (holders, shadowed) = self.holders(&level, name)?;
        let mut kept = Vec::new();
        for &index in &holders {
            let dir = level[index].as_ref().ok_or(FilesystemError::NotFound)?;
            if !self.is_upper(index) || keeps(dir)? {
                kept.push(index);
            }
        }
        let shown = kept.first().copied().into_iter().chain(shadowed).min();
        if let Some(upper) = self.whiteout_layer(shown)? {
            self.mark(chain, &whiteout(name), upper)?;
        }
        for &index in &holders {
            if !kept.contains(&index) {
                remove(level[index].as_ref().ok_or(FilesystemError::NotFound)?)?;
            }
        }
        Ok(())
    }

    pub(in super::super) fn open(
        &self,
        chain: &mut Chain,
        name: &str,
        options: &OpenOptions,
    ) -> Result<Box<dyn File>, FilesystemError> {
        refuse_reserved(name)?;
        let changes = options.write || options.truncate;
        let level = chain.level().clone();
        match self.lookup(&level, name)? {
            Some((_, found)) if found.file_type == FileType::Directory => {
                Err(FilesystemError::IsADirectory)
            }
            Some(_) if options.create_new => Err(FilesystemError::AlreadyExists),
            Some((index, _)) if self.is_upper(index) => {
                layer_dir(&level[index])?.open(name, options)
            }
            Some((index, _)) if !changes => {
                let file = layer_dir(&level[index])?.open(name, options)?;
                Ok(self.readers.open(&chain.path_of(name), file))
            }
            Some((index, _)) => {
                let upper = self.upper()?;
                self.copy_up_file(chain, name, index, upper)?;
                layer_dir(&chain.level()[upper])?.open(name, options)
            }
            None if options.create || options.create_new => self.create_file(chain, name, options),
            None => Err(FilesystemError::NotFound),
        }
    }

    /// A link pointing at nothing still names an entry, so an exclusive
    /// create finds it there; any other create reaches through it, which
    /// only the Layer holding the link may answer. The new file shadows
    /// whatever the whiteout hid before the whiteout goes, so nothing
    /// shows through in between.
    fn create_file(
        &self,
        chain: &mut Chain,
        name: &str,
        options: &OpenOptions,
    ) -> Result<Box<dyn File>, FilesystemError> {
        let level = chain.level().clone();
        match self.lookup(&level, name)? {
            Some(_) if options.create_new => Err(FilesystemError::AlreadyExists),
            Some((index, _)) if self.is_upper(index) => {
                layer_dir(&level[index])?.open(name, options)
            }
            Some(_) => Err(FilesystemError::CrossesDevices),
            None => {
                let upper = self.upper()?;
                let dir = self.upper_dir(chain, upper)?;
                let file = dir.open(name, options)?;
                Self::unhide(&dir, name);
                Ok(file)
            }
        }
    }

    /// A directory made where a whiteout hid one is marked opaque before
    /// the whiteout goes, so the lower Layers' entries never show in it.
    pub(in super::super) fn create_dir(
        &self,
        chain: &mut Chain,
        name: &str,
    ) -> Result<(), FilesystemError> {
        refuse_reserved(name)?;
        let level = chain.level().clone();
        if self.lookup(&level, name)?.is_some() {
            return Err(FilesystemError::AlreadyExists);
        }
        let upper = self.upper()?;
        let dir = self.upper_dir(chain, upper)?;
        let hid = holds(&dir, &whiteout(name))?;
        dir.create_dir(name)?;
        if hid {
            let marked = dir
                .open_dir(name)
                .and_then(|made| put_marker(&made, OPAQUE));
            if let Err(err) = marked {
                let _ = dir.remove_dir(name);
                return Err(err);
            }
            Self::unhide(&dir, name);
        }
        Ok(())
    }

    /// The upper's copy of the directory may hold only markers; any other
    /// entry there means the view is not what was found empty.
    pub(in super::super) fn remove_dir(
        &self,
        chain: &mut Chain,
        name: &str,
    ) -> Result<(), FilesystemError> {
        refuse_reserved(name)?;
        let level = chain.level().clone();
        let (index, found) = self
            .lookup(&level, name)?
            .ok_or(FilesystemError::NotFound)?;
        if found.file_type != FileType::Directory {
            return Err(FilesystemError::NotADirectory);
        }
        if !self.is_empty(&level, name, (index, found))? {
            return Err(FilesystemError::DirectoryNotEmpty);
        }
        self.withdraw(
            chain,
            name,
            |dir| {
                Ok(dir
                    .open_dir(name)?
                    .entries()?
                    .iter()
                    .any(|entry| !entry.name.starts_with(WHITEOUT)))
            },
            |dir| {
                let inner = dir.open_dir(name)?;
                without_markers(&inner, || dir.remove_dir(name))
            },
        )
    }

    /// A link is removed as itself, wherever it points.
    pub(in super::super) fn remove_file(
        &self,
        chain: &mut Chain,
        name: &str,
    ) -> Result<(), FilesystemError> {
        if self.symlink_metadata(chain.level(), name)?.file_type == FileType::Directory {
            return Err(FilesystemError::IsADirectory);
        }
        self.withdraw(chain, name, |_| Ok(false), |dir| dir.remove_file(name))
    }

    /// A directory moves only when the upper alone holds it; moving one
    /// out of a lower Layer would mean copying up everything
    /// beneath it, so it is refused as crossing devices, as overlayfs
    /// refuses it without `redirect_dir`; so is a link or another kind a
    /// lower Layer holds, which no copy up can make. What the view holds
    /// at `to` is checked as rename(2) checks it, and a directory moved
    /// over one a lower Layer holds is marked opaque before it moves, so
    /// it never lands without the marker. The source is whited out before
    /// it moves, so it never shows from a lower Layer once moved.
    pub(in super::super) fn rename(
        &self,
        from: &mut Chain,
        from_name: &str,
        to: &mut Chain,
        to_name: &str,
        rename: Rename,
    ) -> Result<(), FilesystemError> {
        refuse_reserved(to_name)?;
        let from_level = from.level().clone();
        let moving = self.symlink_metadata(&from_level, from_name)?;
        let (from_path, to_path) = (from.path_of(from_name), to.path_of(to_name));
        if from_path == to_path {
            return Ok(());
        }
        rename.check(moving.file_type)?;
        if is_beneath(&to_path, &from_path) {
            return Err(beneath_itself(moving.file_type));
        }
        let to_level = to.level().clone();
        let replaced = self.lookup(&to_level, to_name)?;
        // Only a directory replaces a directory, and only an empty one.
        match (moving.file_type == FileType::Directory, replaced) {
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
        if moving.file_type == FileType::Directory {
            for (index, dir) in to_level.iter().enumerate() {
                if let Some(dir) = dir {
                    replaces_below |= index != upper && holds(dir, to_name)?;
                }
            }
        }
        let (holders, shadowed) = self.holders(&from_level, from_name)?;
        let lower = holders.iter().copied().find(|&index| index != upper);
        let shown_below = self.whiteout_layer(lower.into_iter().chain(shadowed).min())?;
        match moving.file_type {
            FileType::Directory if holders != [upper] => {
                return Err(FilesystemError::CrossesDevices)
            }
            FileType::File if holders.first() != Some(&upper) => {
                let shown = *holders.first().ok_or(FilesystemError::NotFound)?;
                self.copy_up_file(from, from_name, shown, upper)?
            }
            FileType::Symlink | FileType::Unknown if holders.first() != Some(&upper) => {
                return Err(FilesystemError::CrossesDevices)
            }
            _ => {}
        }
        let target_dir = self.upper_dir(to, upper)?;
        if let Some(upper) = shown_below {
            self.mark(from, &whiteout(from_name), upper)?;
        }
        let source_dir = from.level()[upper]
            .clone()
            .ok_or(FilesystemError::NotFound)?;
        let moving_dir = if moving.file_type == FileType::Directory {
            Some(source_dir.open_dir(from_name)?)
        } else {
            None
        };
        let marks = match &moving_dir {
            Some(dir) => replaces_below && !holds(dir, OPAQUE)?,
            None => false,
        };
        if let (true, Some(dir)) = (marks, &moving_dir) {
            put_marker(dir, OPAQUE)?;
        }
        let moved = || source_dir.rename(from_name, &target_dir, to_name, rename);
        let replaces_dir =
            matches!(replaced, Some((_, found)) if found.file_type == FileType::Directory);
        let moved = if replaces_dir && holds(&target_dir, to_name)? {
            let inner = target_dir.open_dir(to_name)?;
            without_markers(&inner, moved)
        } else {
            moved()
        };
        if let Err(err) = moved {
            if let (true, Some(dir)) = (marks, &moving_dir) {
                let _ = dir.remove_file(OPAQUE);
            }
            return Err(err);
        }
        Self::unhide(&target_dir, to_name);
        Ok(())
    }
}
