//! Listing a directory of the view, as overlayfs's `readdir.c` merges each
//! layer's entries and checks a directory is empty before it goes.

use std::collections::HashSet;

use super::super::super::{DirEntry, FilesystemError, Metadata};
use super::{Level, OverlayFs, OPAQUE, WHITEOUT};

impl OverlayFs {
    pub(in super::super) fn entries(
        &self,
        level: &Level,
    ) -> Result<Vec<DirEntry>, FilesystemError> {
        let mut entries: Vec<DirEntry> = Vec::new();
        let mut taken: HashSet<String> = HashSet::new();
        for dir in level.iter().flatten() {
            let listed = dir.entries()?;
            for entry in &listed {
                if !entry.name.starts_with(WHITEOUT) && taken.insert(entry.name.clone()) {
                    entries.push(entry.clone());
                }
            }
            taken.extend(
                listed
                    .iter()
                    .filter_map(|entry| entry.name.strip_prefix(WHITEOUT))
                    .map(str::to_string),
            );
            if listed.iter().any(|entry| entry.name == OPAQUE) {
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
