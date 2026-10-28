//! Setting the times at a path, as Linux's `utimensat` does.

use super::namei::resolve;
use super::path::Path;
use super::Error;
use crate::filesystem::FilesystemError;

/// A Filesystem keeps no timestamps of the guest's choosing, so setting
/// them is unsupported once the path is known to stand.
pub fn set_times(start: &Path, path: &str) -> Result<(), Error> {
    resolve(start, path, false, false)?.check_directory()?;
    Err(FilesystemError::Unsupported.into())
}
