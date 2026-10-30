//! Setting the times at a path, as Linux's `utimensat` does.

use super::namei::filename_lookup;
use super::path::Path;
use super::Error;
use crate::filesystem::FilesystemError;

/// A Filesystem keeps no timestamps of the guest's choosing, so setting
/// them is unsupported once the path is known to stand.
pub fn do_utimes(start: &Path, path: &str) -> Result<(), Error> {
    filename_lookup(start, path, false, false)?.check_directory()?;
    Err(FilesystemError::Unsupported.into())
}
