//! Finding what the view shows at a name, as overlayfs's `namei.c` looks
//! a name up in each layer and merges the directories it finds.

use std::sync::Arc;

use super::super::super::{FileType, FilesystemError, InodeOperations, Metadata};
use super::super::inode::{ovl_inode_init, OverlayInode};
use super::{entry, layer_dir, Level, OverlayFs, Shown, UPPER};
use crate::vfs::{Dentry, Inode};

impl OverlayFs {
    /// The topmost Layer holding an entry at `name` in the directory
    /// `level`, and that entry's kind.
    fn lookup_layer(
        &self,
        level: &Level,
        name: &str,
    ) -> Result<Option<(usize, FileType)>, FilesystemError> {
        for (index, dir) in level.iter().enumerate() {
            let Some(dir) = dir else {
                continue;
            };
            match entry(dir, name)? {
                Shown::Entry(kind) => return Ok(Some((index, kind))),
                Shown::Whiteout => break,
                Shown::Nothing => {}
            }
        }
        Ok(None)
    }

    /// The directory `name` in `level`, from the topmost Layer at `top` showing
    /// it: each Layer's own directory there, merged downward until a Layer holds
    /// something else there, hides it, or marks its directory opaque, as
    /// overlayfs merges only directories.
    fn lookup_layers(
        &self,
        level: &Level,
        name: &str,
        top: usize,
    ) -> Result<Level, FilesystemError> {
        let mut merged = vec![None; level.len()];
        for (index, dir) in level.iter().enumerate().skip(top) {
            let Some(dir) = dir else {
                continue;
            };
            match entry(dir, name)? {
                Shown::Entry(FileType::Directory) => {
                    let inner = dir.open_dir(name)?;
                    let opaque = inner.is_opaque();
                    merged[index] = Some(inner);
                    if opaque {
                        break;
                    }
                }
                Shown::Entry(_) | Shown::Whiteout => break,
                Shown::Nothing => {}
            }
        }
        Ok(merged)
    }

    /// What the view reports for the directory `level`, as `ovl_getattr`
    /// reports a merged one: what its topmost Layer reports, under the identity
    /// of the topmost lower Layer showing it, as `ovl_path_lower` finds it, so
    /// copying it up leaves the identity as it was, and one name.
    pub(in super::super) fn dir_metadata(
        &self,
        level: &Level,
    ) -> Result<Metadata, FilesystemError> {
        let mut shown = level.iter().flatten();
        let topmost = shown.next().ok_or(FilesystemError::NotFound)?.metadata()?;
        Ok(match shown.next() {
            Some(lower) => {
                let lower = lower.metadata()?;
                topmost.with_identity(lower.dev, lower.ino).with_nlink(1)
            }
            None => topmost,
        })
    }

    /// The inode of what the view shows at `name`, answered for by `ops_of` of
    /// its kind, keeping what the lookup found, as `ovl_lookup` fills
    /// `ovl_inode_params` for `ovl_get_inode` under the directory's lock. A
    /// directory one Layer shows stays unmerged, so it keeps what that Layer's
    /// lookup recorded and nothing more is asked of the Layer.
    pub(in super::super) fn lookup(
        &self,
        level: &Level,
        name: &str,
        ops_of: impl Fn(FileType) -> Arc<dyn InodeOperations>,
    ) -> Result<Option<Inode>, FilesystemError> {
        let Some((index, kind)) = self.lookup_layer(level, name)? else {
            return Ok(None);
        };
        let ops = ops_of(kind);
        if kind != FileType::Directory {
            let real = layer_dir(&level[index])?.real(name)?;
            let inode = self.file_inode(index, &real, ops)?;
            ovl_inode_init(&inode, OverlayInode::new(Vec::new(), Some(real)));
            return Ok(Some(inode));
        }
        let merged = self.lookup_layers(level, name, index)?;
        let inode = match merged.iter().flatten().count() {
            1 => {
                let real = layer_dir(&merged[index])?.dentry();
                let found = real.inode().ok_or(FilesystemError::NotFound)?;
                found.with_ops(ops)
            }
            _ => Inode::new(&self.dir_metadata(&merged)?, ops),
        };
        ovl_inode_init(&inode, OverlayInode::new(merged, None));
        Ok(Some(inode))
    }

    /// The inode of the entry `real` in the Layer at `index`, answered for by
    /// `ops`. A hard link in a lower Layer keeps no identity beneath an upper,
    /// as `ovl_hash_bylower` shares no inode for one a copy up would break, so
    /// its names stay two entries of the view. With no upper nothing copies up.
    fn file_inode(
        &self,
        index: usize,
        real: &Dentry,
        ops: Arc<dyn InodeOperations>,
    ) -> Result<Inode, FilesystemError> {
        let found = real.inode().ok_or(FilesystemError::NotFound)?;
        let breaks = self.has_upper() && index != UPPER && real.getattr()?.nlink > 1;
        Ok(match breaks {
            true => Inode::new(&Metadata::new(found.kind(), 0), ops),
            false => found.with_ops(ops),
        })
    }

    /// `name` spelled as the topmost Layer showing it stores it, the
    /// case-exact name `d_add_ci` takes; a name no Layer shows keeps its
    /// spelling.
    pub(in super::super) fn case_exact_name(
        &self,
        level: &Level,
        name: &str,
    ) -> Result<String, FilesystemError> {
        match self.lookup_layer(level, name)? {
            Some((index, _)) => level[index]
                .as_ref()
                .ok_or(FilesystemError::NotFound)?
                .lookup(name),
            None => Ok(name.to_string()),
        }
    }
}
