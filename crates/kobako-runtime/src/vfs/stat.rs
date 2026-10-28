//! Looking at what stands at a path, as Linux's `fstatat` and
//! `readlinkat` do.

use super::namei::{filename_lookup, names_directory};
use super::path::Path;
use super::times::mask_times;
use super::Error;
use crate::filesystem::{FilesystemError, Metadata};

/// What `path` beneath `start` names, a link at its end followed.
pub fn metadata(start: &Path, path: &str) -> Result<Metadata, Error> {
    stat(start, path, true)
}

/// What `path` beneath `start` names, a link at its end reported as
/// itself unless a trailing `/` asks for the directory it leads to, as
/// Linux's `lstat` answers.
pub fn symlink_metadata(start: &Path, path: &str) -> Result<Metadata, Error> {
    stat(start, path, names_directory(path))
}

fn stat(start: &Path, path: &str, follow: bool) -> Result<Metadata, Error> {
    let mut nd = filename_lookup(start, path, follow, false)?;
    // A link the walk did not follow at the end is one swapped in since,
    // so it is reported as itself rather than followed.
    nd.check_directory()?;
    Ok(mask_times(nd.found()?, start.profile()))
}

/// A guest never reads where a link points, so reading one is
/// unsupported once the path is known to stand.
pub fn read_link(start: &Path, path: &str) -> Result<(), Error> {
    filename_lookup(start, path, false, false)?.check_directory()?;
    Err(FilesystemError::Unsupported.into())
}
