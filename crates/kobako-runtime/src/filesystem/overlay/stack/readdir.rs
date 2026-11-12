//! Listing a directory of the view, as overlayfs's `readdir.c` merges each
//! layer's entries and checks a directory is empty before it goes.

use std::collections::HashSet;

use super::super::super::{DirEntry, Errno, FileType};
use super::{Level, OverlayFs, UPPER};
use crate::vfs::Dentry;

/// What a directory of the view lists, and whether its upper holds a
/// whiteout, which overlayfs's `ovl_dir_read_merged` keeps on its list.
struct MergedEntries {
    entries: Vec<DirEntry>,
    upper_whiteouts: bool,
}

/// Each Layer's entries topmost first, a name a Layer above shows or whites
/// out taken once.
fn read_merged(level: &Level) -> Result<MergedEntries, Errno> {
    let mut merged = MergedEntries {
        entries: Vec::new(),
        upper_whiteouts: false,
    };
    let mut taken: HashSet<String> = HashSet::new();
    for (index, dir) in level.iter().enumerate() {
        let Some(dir) = dir else { continue };
        for entry in dir.entries()? {
            // A whiteout lists as a device, as overlayfs's `ovl_fill_merge`
            // reads `DT_CHR` before asking.
            let whiteout = entry.file_type == FileType::Chrdev && dir.is_whiteout(&entry.name);
            merged.upper_whiteouts |= whiteout && index == UPPER;
            if taken.insert(entry.name.clone()) && !whiteout {
                merged.entries.push(entry);
            }
        }
    }
    Ok(merged)
}

impl OverlayFs {
    /// Each Layer's entries topmost first, a name a Layer above shows or
    /// whites out taken once. A lookup left out what an opaque directory hides.
    pub(in super::super) fn dir_read_merged(&self, level: &Level) -> Result<Vec<DirEntry>, Errno> {
        Ok(read_merged(level)?.entries)
    }

    /// Whether the directory `dentry` names shows no entry, `None` where it
    /// shows one, and otherwise whether its upper holds whiteouts to clear
    /// first, as `ovl_check_empty_dir` hands `ovl_clear_empty` its list.
    pub(super) fn check_empty_dir(&self, dentry: &Dentry) -> Result<Option<bool>, Errno> {
        let merged = read_merged(&self.dir(dentry)?.level())?;
        Ok(merged.entries.is_empty().then_some(merged.upper_whiteouts))
    }
}
