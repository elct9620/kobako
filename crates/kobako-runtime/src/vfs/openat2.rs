//! What an open asks for and what it reaches, as Linux's `openat2` takes
//! an `open_how` and `vfs_open` holds what the walk opened.

use super::file::File;
use super::namei::Nameidata;
use super::path::Path;
use super::Error;
use super::FilesystemError;

/// How `InodeOperations::open` opens a file.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct OpenOptions {
    pub read: bool,
    pub write: bool,
    /// Create the file when it does not exist.
    pub create: bool,
    /// Create the file, refusing one that already exists.
    pub create_new: bool,
    /// Empty the file once it is open.
    pub truncate: bool,
    /// Open a directory, to list it, as `O_DIRECTORY` does.
    pub directory: bool,
}

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
    /// Make each write durable before it answers, as `O_SYNC` does.
    pub sync: bool,
}

/// Refuse flags no open can mean before any path is looked at, as Linux's
/// `build_open_flags` refuses `O_DIRECTORY` with `O_CREAT`.
pub(crate) fn build_open_flags(how: &OpenHow) -> Result<(), FilesystemError> {
    match how.directory && how.create {
        true => Err(FilesystemError::InvalidInput),
        false => Ok(()),
    }
}

/// The directory `nd` ends at, opened now, as Linux's `vfs_open` holds
/// what it opened, so the descriptor follows it wherever it moves.
pub(crate) fn vfs_open(start: &Path, nd: Nameidata, how: &OpenHow) -> Result<File, Error> {
    let dentry = match nd.name.as_str() {
        "" => nd.dentry().clone(),
        _ => {
            let _held = nd.dentry().inode_lock_shared()?;
            let dentry = nd.last_dentry()?;
            let inode = dentry.inode().ok_or(FilesystemError::NotFound)?;
            dentry.can_lookup(inode.ops())?;
            dentry
        }
    };
    Ok(File::directory(start.with_dentry(dentry), nd.cred.clone())?.with_flags(how))
}
