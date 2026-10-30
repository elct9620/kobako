//! What an open asks for and what it reaches, as Linux's `openat2` takes
//! an `open_how` and `vfs_open` holds what the walk opened.

use super::file::File;
use super::namei::{d_can_lookup, Nameidata};
use super::path::Path;
use super::Error;
use crate::filesystem::FilesystemError;

/// How to open a path, as Linux's `struct open_how` carries `openat2`'s
/// flags.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct OpenHow {
    pub read: bool,
    pub write: bool,
    /// Create a file where nothing stands, as `O_CREAT` does.
    pub create: bool,
    /// Refuse what already stands there, as `O_EXCL` does.
    pub exclusive: bool,
    /// Empty the file once it is open, as `O_TRUNC` does.
    pub truncate: bool,
    /// Refuse anything but a directory, as `O_DIRECTORY` does.
    pub directory: bool,
    /// Follow a link at the end of the path.
    pub follow: bool,
    /// Land each write at the end, as `O_APPEND` does.
    pub append: bool,
    /// Ask not to wait, as `O_NONBLOCK` does.
    pub nonblock: bool,
}

/// The directory `nd` ends at, opened now, as Linux's `vfs_open` holds
/// what it opened, so the descriptor follows it wherever it moves.
pub(crate) fn vfs_open(start: &Path, nd: Nameidata) -> Result<File, Error> {
    let dentry = match nd.name.as_str() {
        "" => nd.dentry().clone(),
        _ => {
            let _held = nd.dentry().inode_lock_shared()?;
            let dentry = nd.walked()?;
            let inode = dentry.inode().ok_or(FilesystemError::NotFound)?;
            d_can_lookup(inode.ops(), &dentry)?;
            dentry
        }
    };
    Ok(File::directory(start.with_dentry(dentry), nd.cred.clone())?)
}
