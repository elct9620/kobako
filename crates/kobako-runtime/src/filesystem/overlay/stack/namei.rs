//! Finding what the view shows at a name, as overlayfs's `namei.c` looks
//! a name up in each layer and merges the directories it finds.

use std::sync::Arc;

use super::super::super::{FileType, FilesystemError, InodeOperations, Metadata};
use super::super::inode::{ovl_inode_init, OverlayInode};
use super::{entry, layer_dir, shows_below, sole_layer, Level, OverlayFs};
use crate::vfs::Inode;

impl OverlayFs {
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

    /// The inode of what the view shows at `name`, answered for by `ops_of` of
    /// its kind, keeping what the lookup found, as `ovl_lookup` fills
    /// `ovl_inode_params` for `ovl_get_inode` under the directory's lock. A
    /// directory one Layer shows stays unmerged, so nothing more is asked of that
    /// Layer, which shows nothing where it holds a whiteout, as
    /// `ovl_lookup_single` stops at one in any layer.
    pub(in super::super) fn view_entry(
        &self,
        level: &Level,
        name: &str,
        ops_of: impl Fn(FileType) -> Arc<dyn InodeOperations>,
    ) -> Result<Option<Inode>, FilesystemError> {
        if let Some((index, dir)) = sole_layer(level) {
            if dir.is_whiteout(name) {
                return Ok(None);
            }
            let real = dir.real(name)?;
            let Some(found) = real.inode() else {
                return Ok(None);
            };
            let inode = found.with_ops(ops_of(found.kind()));
            let kept = match found.kind() {
                FileType::Directory => {
                    let mut only = vec![None; level.len()];
                    only[index] = Some(dir.with_dentry(real));
                    OverlayInode::new(only, None)
                }
                _ => OverlayInode::new(Vec::new(), Some(real)),
            };
            ovl_inode_init(&inode, kept);
            return Ok(Some(inode));
        }
        let Some((index, found)) = self.lookup(level, name)? else {
            return Ok(None);
        };
        if found.file_type != FileType::Directory {
            let real = layer_dir(&level[index])?.real(name)?;
            let inode = Inode::new(&found, ops_of(found.file_type));
            ovl_inode_init(&inode, OverlayInode::new(Vec::new(), Some(real)));
            return Ok(Some(inode));
        }
        let merged = self.lookup_layers(level, name, (index, found))?;
        let inode = Inode::new(&self.dir_metadata(&merged)?, ops_of(FileType::Directory));
        ovl_inode_init(&inode, OverlayInode::new(merged, None));
        Ok(Some(inode))
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
}
