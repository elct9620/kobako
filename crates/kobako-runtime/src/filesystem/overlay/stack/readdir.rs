//! Listing a directory of the view, as overlayfs's `readdir.c` merges each
//! layer's entries and checks a directory is empty before it goes.

use std::collections::HashSet;

use super::super::super::{DirEntry, FileType, FilesystemError, Metadata};
use super::{Level, OverlayFs};

impl OverlayFs {
    /// Each Layer's entries topmost first, a name a Layer above shows or
    /// whites out taken once, down to the first directory marked opaque.
    pub(in super::super) fn entries(
        &self,
        level: &Level,
    ) -> Result<Vec<DirEntry>, FilesystemError> {
        let mut entries: Vec<DirEntry> = Vec::new();
        let mut taken: HashSet<String> = HashSet::new();
        for dir in level.iter().flatten() {
            for entry in dir.entries()? {
                // A whiteout lists as a device, as overlayfs's
                // `ovl_fill_merge` reads `DT_CHR` before asking.
                let whiteout = entry.file_type == FileType::Unknown && dir.is_whiteout(&entry.name);
                if taken.insert(entry.name.clone()) && !whiteout {
                    entries.push(entry);
                }
            }
            if dir.is_opaque() {
                break;
            }
        }
        Ok(entries)
    }

    /// Whether the directory `name` in `level` shows no entry.
    pub(super) fn is_empty(
        &self,
        level: &Level,
        name: &str,
        found: (usize, Metadata),
    ) -> Result<bool, FilesystemError> {
        Ok(self
            .entries(&self.lookup_layers(level, name, found)?)?
            .is_empty())
    }
}
