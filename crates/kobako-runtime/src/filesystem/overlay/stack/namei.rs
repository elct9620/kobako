//! Finding what the view shows at a name, as overlayfs's `namei.c` looks
//! a name up in each layer and merges the directories it finds.

use super::super::super::{FileType, FilesystemError, Metadata};
use super::{entry, layer_dir, shows_below, sole_layer, Chain, Level, OverlayFs, Step};

impl OverlayFs {
    /// Walk `chain` into each of `names` in turn, each a directory the view
    /// shows.
    pub(in super::super) fn descend<'a>(
        &self,
        chain: &mut Chain,
        names: impl IntoIterator<Item = &'a str>,
    ) -> Result<(), FilesystemError> {
        for name in names {
            self.enter(chain, name)?;
        }
        Ok(())
    }

    /// Step `chain` into the directory `name`, refusing a link standing
    /// there as a loop, since the runtime follows a link itself, in the
    /// view.
    pub(in super::super) fn enter(
        &self,
        chain: &mut Chain,
        name: &str,
    ) -> Result<(), FilesystemError> {
        let level = chain.level();
        if let Some((index, dir)) = sole_layer(level) {
            let mut only = vec![None; level.len()];
            only[index] = Some(dir.open_dir(name)?);
            chain.steps.push(Step {
                name: name.to_string(),
                level: only,
            });
            return Ok(());
        }
        let (index, found) = self.lookup(level, name)?.ok_or(FilesystemError::NotFound)?;
        let step = match found.file_type {
            FileType::Directory => Step {
                name: name.to_string(),
                level: self.lookup_layers(level, name, (index, found))?,
            },
            FileType::Symlink => return Err(FilesystemError::FilesystemLoop),
            _ => return Err(FilesystemError::NotADirectory),
        };
        chain.steps.push(step);
        Ok(())
    }

    /// The topmost Layer holding an entry at `name` in the directory
    /// `level`, and that entry, a link reported as itself.
    pub(super) fn lookup(
        &self,
        level: &Level,
        name: &str,
    ) -> Result<Option<(usize, Metadata)>, FilesystemError> {
        for (index, dir) in level.iter().enumerate() {
            let Some(dir) = dir else {
                continue;
            };
            if let Some(found) = entry(dir, name)? {
                return Ok(Some((index, found)));
            }
            if shows_below(level, index) && dir.is_whiteout(name) {
                break;
            }
        }
        Ok(None)
    }

    /// The directory `name` in `level`, found as `found` names it first: each
    /// Layer's own directory there, merged downward until a Layer holds
    /// something else there, hides it, or marks its directory opaque, as
    /// overlayfs merges only directories.
    pub(super) fn lookup_layers(
        &self,
        level: &Level,
        name: &str,
        found: (usize, Metadata),
    ) -> Result<Level, FilesystemError> {
        let mut merged = vec![None; level.len()];
        for (index, dir) in level.iter().enumerate().skip(found.0) {
            let Some(dir) = dir else {
                continue;
            };
            let below = shows_below(level, index);
            let standing = if index == found.0 {
                Some(found.1)
            } else {
                entry(dir, name)?
            };
            match standing {
                Some(metadata) if metadata.file_type == FileType::Directory => {
                    let inner = dir.open_dir(name)?;
                    let opaque = below && inner.is_opaque();
                    merged[index] = Some(inner);
                    if opaque {
                        break;
                    }
                }
                Some(_) => break,
                None => {}
            }
            if below && dir.is_whiteout(name) {
                break;
            }
        }
        Ok(merged)
    }

    /// What the view reports for the directory `level`, as overlayfs
    /// reports a merged directory: the identity of the lowest Layer showing
    /// it, so copying it up into the upper leaves the identity as it
    /// was, the times of the topmost, and one name where more than one
    /// Layer shows it.
    pub(in super::super) fn dir_metadata(
        &self,
        level: &Level,
    ) -> Result<Metadata, FilesystemError> {
        let mut shown = level.iter().flatten();
        let topmost = shown.next().ok_or(FilesystemError::NotFound)?.metadata()?;
        let lowest = match shown.last() {
            Some(lowest) => lowest.metadata()?.with_nlink(1),
            None => topmost,
        };
        Ok(Metadata {
            accessed: topmost.accessed,
            modified: topmost.modified,
            changed: topmost.changed,
            ..lowest
        })
    }

    /// `found` at `name` in the Layer at `index`, as the view reports it: a
    /// directory takes the identity of the lowest Layer showing it.
    fn merge_identity(
        &self,
        level: &Level,
        name: &str,
        index: usize,
        found: Metadata,
    ) -> Result<Metadata, FilesystemError> {
        if found.file_type != FileType::Directory {
            return Ok(found);
        }
        self.dir_metadata(&self.lookup_layers(level, name, (index, found))?)
    }

    /// What the Layer showing `name` holds there, a link it holds
    /// reported as one even where it points at nothing.
    pub(in super::super) fn symlink_metadata(
        &self,
        level: &Level,
        name: &str,
    ) -> Result<Metadata, FilesystemError> {
        let (index, found) = self.lookup(level, name)?.ok_or(FilesystemError::NotFound)?;
        self.merge_identity(level, name, index, found)
    }

    /// `name` spelled as the topmost Layer showing it stores it, the
    /// case-exact name `d_add_ci` takes; a name no Layer shows keeps its
    /// spelling. A directory one Layer alone shows asks that Layer, as it
    /// walks there.
    pub(in super::super) fn case_exact_name(
        &self,
        level: &Level,
        name: &str,
    ) -> Result<String, FilesystemError> {
        if let Some((_, dir)) = sole_layer(level) {
            return dir.lookup(name);
        }
        match self.lookup(level, name)? {
            Some((index, _)) => level[index]
                .as_ref()
                .ok_or(FilesystemError::NotFound)?
                .lookup(name),
            None => Ok(name.to_string()),
        }
    }

    /// Where the link the view shows at `name` points, as the Layer
    /// holding it reads it.
    pub(in super::super) fn read_link(
        &self,
        level: &Level,
        name: &str,
    ) -> Result<String, FilesystemError> {
        match self.lookup(level, name)?.ok_or(FilesystemError::NotFound)? {
            (index, found) if found.file_type == FileType::Symlink => {
                layer_dir(&level[index])?.read_link(name)
            }
            _ => Err(FilesystemError::InvalidInput),
        }
    }
}
